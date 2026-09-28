//! Ring 3, a versioned syscall ABI, and validated user-pointer copies (dossier
//! sections 9 and 10, roadmap P0 step 4).
//!
//! A tiny user routine is written into a fresh frame and mapped user/executable/
//! read-only above the identity window. The SYSCALL MSRs are programmed, the
//! kernel drops to CPL 3 with `iretq`, and the routine makes a sequence of real
//! syscalls that each return through `sysret`:
//!
//! - `SYS_VERSION` returns the ABI version;
//! - `SYS_ADD` returns the sum of two arguments;
//! - `SYS_WRITE` passes a user pointer and length that the kernel validates
//!   (canonical, inside the user page, bounded) and copies in with `stac`/`clac`
//!   around the access so SMAP is honoured;
//! - `SYS_EXIT` returns control to the kernel to report.
//!
//! Interrupts stay masked throughout, so no asynchronous entry from CPL 3
//! happens. The CPL3 code still runs on a distinct user `GS` base, and the
//! syscall entry uses `swapgs` to reach the kernel per-CPU block rather than
//! trusting whatever `GS` the user held - the discipline a preemptive user
//! scheduler will depend on, proved here by `AW_SWAPGS_PROOF_OK`.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use aw_x86_paging::PageTableFlags;

use crate::interrupts::{USER_CODE_SELECTOR_RAW, USER_DATA_SELECTOR_RAW};
use crate::local_apic::{rdmsr, wrmsr};
use crate::virtio_blk::BlkDevice;
use crate::{
    debug_write, debug_write_hex_u64, debug_write_u64, fat16, frame_allocator, interrupts,
    page_mapper,
};

const IA32_EFER: u32 = 0xc000_0080;
const IA32_STAR: u32 = 0xc000_0081;
const IA32_LSTAR: u32 = 0xc000_0082;
const IA32_FMASK: u32 = 0xc000_0084;
const IA32_GS_BASE: u32 = 0xc000_0101;
const IA32_KERNEL_GS_BASE: u32 = 0xc000_0102;
const EFER_SYSCALL_ENABLE: u64 = 1 << 0;

const USER_CODE_VA: u64 = 0x2_0000_0000;
const USER_STACK_VA: u64 = 0x2_0000_2000;
const USER_STACK_TOP: u64 = USER_STACK_VA + 0x1000;
const USER_PAGE_END: u64 = USER_CODE_VA + 0x1000;

/// Syscall numbers - the start of a stable, versioned ABI (dossier section 9).
const SYS_VERSION: u64 = 0;
const SYS_ADD: u64 = 1;
const SYS_WRITE: u64 = 2;
/// Report a scalar back to the kernel, no pointer involved: how a loaded userland
/// program signals it ran, for the loader proof.
const SYS_REPORT: u64 = 3;
const SYS_EXIT: u64 = 0xff;
const ABI_VERSION: u64 = 1;

/// Largest user->kernel copy this proof accepts.
const MAX_WRITE: usize = 64;

/// The user routine, hand-assembled, position independent:
/// SYS_ADD(2,3); SYS_WRITE(USER_CODE_VA+60, 13); SYS_EXIT; then a parking loop;
/// then the 13-byte message "HELLO-SYSCALL".
const USER_CODE: [u8; 73] = [
    0x48, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1  (SYS_ADD)
    0x48, 0xc7, 0xc7, 0x02, 0x00, 0x00, 0x00, // mov rdi, 2
    0x48, 0xc7, 0xc6, 0x03, 0x00, 0x00, 0x00, // mov rsi, 3
    0x0f, 0x05, // syscall -> rax = 5
    0x48, 0xc7, 0xc0, 0x02, 0x00, 0x00, 0x00, // mov rax, 2  (SYS_WRITE)
    0x48, 0xbf, 0x3c, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, // mov rdi, USER_CODE_VA+60
    0x48, 0xc7, 0xc6, 0x0d, 0x00, 0x00, 0x00, // mov rsi, 13
    0x0f, 0x05, // syscall -> rax = 13
    0x48, 0xc7, 0xc0, 0xff, 0x00, 0x00, 0x00, // mov rax, 0xff (SYS_EXIT)
    0x0f, 0x05, // syscall (does not return to user)
    0xeb, 0xfe, // jmp . (parking, never reached)
    0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x2d, 0x53, 0x59, 0x53, 0x43, 0x41, 0x4c,
    0x4c, // "HELLO-SYSCALL"
];

/// What SYS_WRITE must deliver, for the proof.
const EXPECTED_WRITE: &[u8] = b"HELLO-SYSCALL";

#[repr(C, align(16))]
struct KernelStack([u8; 16 * 1024]);
static mut RING0_STACK: KernelStack = KernelStack([0; 16 * 1024]);

fn ring0_stack_top() -> u64 {
    let base = core::ptr::addr_of!(RING0_STACK) as u64;
    (base + 16 * 1024) & !0xf_u64
}

// Shared with the assembly. SAVED_KERNEL_* return control to the kernel on exit;
// SAVED_USER_* let the entry stub sysret back to the user for a normal syscall.
#[used]
static SAVED_KERNEL_RSP: AtomicU64 = AtomicU64::new(0);
#[used]
static SAVED_KERNEL_RESUME: AtomicU64 = AtomicU64::new(0);
#[used]
static KERNEL_SYSCALL_RSP: AtomicU64 = AtomicU64::new(0);
#[used]
static SAVED_USER_RSP: AtomicU64 = AtomicU64::new(0);
#[used]
static SAVED_USER_RIP: AtomicU64 = AtomicU64::new(0);
#[used]
static SAVED_USER_RFLAGS: AtomicU64 = AtomicU64::new(0);
#[used]
static SYSCALL_EXIT: AtomicU8 = AtomicU8::new(0);

/// Two distinct GS bases for the swapgs proof. The kernel area's first qword is a
/// recognizable magic; the user area's is zero. The syscall entry runs with the
/// user area live and must `swapgs` to the kernel area, so `gs:[0]` reads the
/// magic if the swap happened and null if it did not - a clean failure either
/// way, never a user-controlled pointer dereference. These stand in for a real
/// per-CPU base, which is not installed yet this early in boot; the point proved
/// is the swap itself, not what the kernel GS ultimately points at.
const KERNEL_GS_MAGIC: u64 = 0x0000_1111_2222_3333;

#[repr(C, align(64))]
struct GsArea([u64; 8]);
static mut KERNEL_GS_AREA: GsArea = GsArea([KERNEL_GS_MAGIC, 0, 0, 0, 0, 0, 0, 0]);
static mut USER_GS_AREA: GsArea = GsArea([0; 8]);
/// The value the syscall handler read from `gs:[0]` after `swapgs`.
static OBSERVED_KERNEL_GS: AtomicU64 = AtomicU64::new(0);

// The value a loaded userland program reported through SYS_REPORT, for the loader
// proof.
static REPORTED_VALUE: AtomicU64 = AtomicU64::new(0);
static REPORTED_SEEN: AtomicBool = AtomicBool::new(false);

// Results captured for the proof.
static ADD_RESULT: AtomicU64 = AtomicU64::new(0);
static ADD_SEEN: AtomicBool = AtomicBool::new(false);
static WRITE_COPIED: AtomicU64 = AtomicU64::new(u64::MAX);
static mut WRITE_BUFFER: [u8; MAX_WRITE] = [0; MAX_WRITE];

const USER_DATA_SELECTOR: u64 = USER_DATA_SELECTOR_RAW as u64;
const USER_CODE_SELECTOR: u64 = USER_CODE_SELECTOR_RAW as u64;
const USER_RFLAGS: u64 = 0x2;

core::arch::global_asm!(
    ".global aw_enter_ring3",
    ".global aw_syscall_entry",
    // fn aw_enter_ring3(user_rip = rdi, user_rsp = rsi)
    "aw_enter_ring3:",
    "    lea rax, [rip + aw_ring3_resume]",
    "    mov [rip + {saved_resume}], rax",
    "    mov [rip + {saved_rsp}], rsp",
    "    push {user_ss}",
    "    push rsi",
    "    push {user_flags}",
    "    push {user_cs}",
    "    push rdi",
    "    iretq",
    "aw_ring3_resume:",
    "    ret",
    // syscall entry: rax = number, rdi/rsi = args, rcx = user rip, r11 = rflags.
    "aw_syscall_entry:",
    // The CPU entered with the user's GS still live. swapgs installs the kernel
    // GS base this CPU stashed in IA32_KERNEL_GS_BASE, so gs:[0] reaches the
    // per-CPU block instead of anything user-controlled (dossier sections 9-10).
    "    swapgs",
    "    mov [rip + {user_rsp}], rsp",
    "    mov [rip + {user_rip}], rcx",
    "    mov [rip + {user_rflags}], r11",
    "    mov rsp, [rip + {kernel_rsp}]",
    // Marshal to the System V dispatch(nr, a0, a1, a2). rcx (the user rip) is
    // already saved, so it can carry the third argument.
    "    mov rcx, rdx",
    "    mov rdx, rsi",
    "    mov rsi, rdi",
    "    mov rdi, rax",
    "    call {dispatch}",
    // Never hand kernel scratch values back to CPL3: clear every caller-saved
    // register the dispatcher may have left behind (rax is the result, rcx/r11
    // are reloaded below).
    "    xor edi, edi",
    "    xor esi, esi",
    "    xor edx, edx",
    "    xor r8d, r8d",
    "    xor r9d, r9d",
    "    xor r10d, r10d",
    // rax holds the result. Exit returns to the kernel; otherwise sysret back.
    "    cmp byte ptr [rip + {exit_flag}], 0",
    "    jne 2f",
    "    mov rcx, [rip + {user_rip}]",
    "    mov r11, [rip + {user_rflags}]",
    "    mov rsp, [rip + {user_rsp}]",
    // Restore the user GS base before dropping back to CPL3.
    "    swapgs",
    "    sysretq",
    // Exit path: control returns to the kernel, which wants the kernel GS that
    // the entry swapgs already installed, so it is deliberately not swapped back.
    "2:",
    "    mov rsp, [rip + {saved_rsp}]",
    "    jmp [rip + {saved_resume}]",
    saved_resume = sym SAVED_KERNEL_RESUME,
    saved_rsp = sym SAVED_KERNEL_RSP,
    kernel_rsp = sym KERNEL_SYSCALL_RSP,
    user_rsp = sym SAVED_USER_RSP,
    user_rip = sym SAVED_USER_RIP,
    user_rflags = sym SAVED_USER_RFLAGS,
    exit_flag = sym SYSCALL_EXIT,
    dispatch = sym aw_syscall_dispatch,
    user_ss = const USER_DATA_SELECTOR,
    user_cs = const USER_CODE_SELECTOR,
    user_flags = const USER_RFLAGS,
);

unsafe extern "C" {
    fn aw_enter_ring3(user_rip: u64, user_rsp: u64);
    fn aw_syscall_entry();
}

fn smap_enabled() -> bool {
    let cr4: u64;
    // SAFETY: reading CR4 at CPL0 has no side effects.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags))
    };
    cr4 & (1 << 21) != 0
}

/// Copy `len` bytes from a validated user address into the kernel buffer,
/// honouring SMAP with `stac`/`clac`. Returns the number of bytes copied.
///
/// # Safety
/// `user_ptr..user_ptr+len` must be a readable user page (validated by the
/// caller) and `len <= MAX_WRITE`.
unsafe fn copy_from_user(user_ptr: u64, len: usize) -> usize {
    let smap = smap_enabled();
    if smap {
        // SAFETY: permit supervisor access to user pages for this copy only.
        unsafe { core::arch::asm!("stac", options(nomem, nostack)) };
    }
    // SAFETY: the range was validated as an in-bounds user page.
    unsafe {
        let dst = core::ptr::addr_of_mut!(WRITE_BUFFER) as *mut u8;
        for index in 0..len {
            dst.add(index)
                .write_volatile((user_ptr as *const u8).add(index).read_volatile());
        }
    }
    if smap {
        // SAFETY: re-arm SMAP.
        unsafe { core::arch::asm!("clac", options(nomem, nostack)) };
    }
    len
}

/// The syscall dispatcher. Returns the value the user receives in `rax`.
#[unsafe(no_mangle)]
extern "C" fn aw_syscall_dispatch(number: u64, arg0: u64, arg1: u64, arg2: u64) -> u64 {
    // Record, once, what gs:[0] holds. The entry stub ran swapgs before calling
    // here, so this is the kernel area's magic if the swap worked, and null (from
    // USER_GS_AREA) if it did not.
    if OBSERVED_KERNEL_GS.load(Ordering::Relaxed) == 0 {
        let gs0: u64;
        // SAFETY: reading a GS-relative qword has no side effects; offset 0 of
        // the kernel GS area holds KERNEL_GS_MAGIC once swapgs has run.
        unsafe {
            core::arch::asm!("mov {}, gs:[0]", out(reg) gs0, options(nostack, preserves_flags, readonly));
        }
        OBSERVED_KERNEL_GS.store(gs0, Ordering::Relaxed);
    }

    match number {
        SYS_VERSION => ABI_VERSION,
        SYS_ADD => {
            let sum = arg0.wrapping_add(arg1);
            ADD_RESULT.store(sum, Ordering::Relaxed);
            ADD_SEEN.store(true, Ordering::Relaxed);
            sum
        }
        SYS_WRITE => {
            let len = arg1 as usize;
            // Validate the user pointer: bounded length, and the whole range
            // inside the user code page (dossier section 9).
            let end = arg0.checked_add(arg1);
            if len > MAX_WRITE || arg0 < USER_CODE_VA || end.is_none_or(|e| e > USER_PAGE_END) {
                return u64::MAX;
            }
            // SAFETY: range validated above; copy honours SMAP.
            let copied = unsafe { copy_from_user(arg0, len) };
            WRITE_COPIED.store(copied as u64, Ordering::Relaxed);
            copied as u64
        }
        SYS_REPORT => {
            REPORTED_VALUE.store(arg0, Ordering::Relaxed);
            REPORTED_SEEN.store(true, Ordering::Relaxed);
            arg0
        }
        SYS_EXIT => {
            SYSCALL_EXIT.store(1, Ordering::Relaxed);
            0
        }
        crate::ipc::SYS_CHANNEL_SEND
        | crate::ipc::SYS_CHANNEL_RECV
        | crate::ipc::SYS_HANDLE_CLOSE => {
            crate::ipc::dispatch(crate::scheduler::current_slot(), number, arg0, arg1, arg2)
        }
        _ => u64::MAX,
    }
}

/// Program the SYSCALL MSRs.
///
/// # Safety
/// CPL0, before Ring 3 is entered.
unsafe fn enable_syscall() {
    // SAFETY: EFER already carries LME/LMA/NXE; add SCE.
    let efer = unsafe { rdmsr(IA32_EFER) };
    unsafe { wrmsr(IA32_EFER, efer | EFER_SYSCALL_ENABLE) };
    // syscall: kernel CS 0x08 (SS 0x10). sysret base 0x20 -> user SS 0x28, CS 0x30.
    let star = (0x0020_u64 << 48) | (0x0008_u64 << 32);
    unsafe { wrmsr(IA32_STAR, star) };
    unsafe { wrmsr(IA32_LSTAR, aw_syscall_entry as *const () as u64) };
    // Clear IF, DF, TF, AC on entry.
    unsafe { wrmsr(IA32_FMASK, (1 << 9) | (1 << 10) | (1 << 8) | (1 << 18)) };
}

/// Enter Ring 3, run the syscall sequence, and prove the ABI.
pub fn prove() {
    debug_write("AW_RING3_BEGIN\n");

    let Some(stack_frame) = frame_allocator::allocate() else {
        debug_write("AW_RING3_FAIL reason=no_stack_frame\n");
        return;
    };
    let stack_flags = PageTableFlags::USER_ACCESSIBLE
        .union(PageTableFlags::WRITABLE)
        .union(PageTableFlags::NO_EXECUTE);
    // SAFETY: CPL0; USER_STACK_VA is unused and the frame was just allocated.
    if unsafe { page_mapper::map_page(USER_STACK_VA, stack_frame, stack_flags) }.is_err() {
        debug_write("AW_RING3_FAIL reason=map_stack\n");
        return;
    }

    let Some(code_frame) = frame_allocator::allocate() else {
        debug_write("AW_RING3_FAIL reason=no_code_frame\n");
        return;
    };
    // SAFETY: write the routine through the frame's identity address, then map
    // it user/executable/read-only.
    unsafe {
        core::ptr::copy_nonoverlapping(USER_CODE.as_ptr(), code_frame as *mut u8, USER_CODE.len());
    }
    if unsafe { page_mapper::map_page(USER_CODE_VA, code_frame, PageTableFlags::USER_ACCESSIBLE) }
        .is_err()
    {
        debug_write("AW_RING3_FAIL reason=map_code\n");
        return;
    }

    // SAFETY: CPL0, single core, before Ring 3 is entered.
    unsafe {
        interrupts::set_bootstrap_rsp0(ring0_stack_top());
        KERNEL_SYSCALL_RSP.store(ring0_stack_top(), Ordering::Relaxed);
        enable_syscall();
    }

    // Arm swapgs: put the kernel GS area in IA32_KERNEL_GS_BASE and a distinct
    // user area in the live GS, so the CPL3 code runs on the user GS and the
    // syscall entry's swapgs must bring in the kernel one. install_bootstrap_per_cpu
    // overwrites the live GS with the real per-CPU base right after this proof.
    let kernel_gs = core::ptr::addr_of!(KERNEL_GS_AREA) as u64;
    let user_gs = core::ptr::addr_of!(USER_GS_AREA) as u64;
    // SAFETY: CPL0; both bases are canonical addresses of live statics, and the
    // kernel makes no GS-relative access between here and the entry swapgs.
    unsafe {
        wrmsr(IA32_KERNEL_GS_BASE, kernel_gs);
        wrmsr(IA32_GS_BASE, user_gs);
    }

    debug_write("AW_RING3_MAP_OK code_va=");
    debug_write_hex_u64(USER_CODE_VA);
    debug_write(" stack_va=");
    debug_write_hex_u64(USER_STACK_VA);
    debug_write("\n");

    // SAFETY: user pages mapped, MSRs and rsp0 set, interrupts masked.
    unsafe { aw_enter_ring3(USER_CODE_VA, USER_STACK_TOP) };

    // Control is back in the kernel after SYS_EXIT. Check the ABI results.
    let add_ok = ADD_SEEN.load(Ordering::Relaxed) && ADD_RESULT.load(Ordering::Relaxed) == 5;
    debug_write("AW_SYSCALL_ADD result=");
    debug_write_u64(ADD_RESULT.load(Ordering::Relaxed));
    debug_write("\n");

    let copied = WRITE_COPIED.load(Ordering::Relaxed);
    // SAFETY: the dispatcher filled WRITE_BUFFER with `copied` bytes.
    let write_ok = copied == EXPECTED_WRITE.len() as u64
        && unsafe {
            let buffer = core::ptr::addr_of!(WRITE_BUFFER) as *const u8;
            (0..EXPECTED_WRITE.len()).all(|i| buffer.add(i).read() == EXPECTED_WRITE[i])
        };
    debug_write("AW_SYSCALL_WRITE copied=");
    debug_write_u64(copied);
    debug_write("\n");

    if add_ok && write_ok {
        debug_write("AW_RING3_PROOF_OK\n");
        debug_write("AW_SYSCALL_ABI_PROOF_OK version=");
        debug_write_u64(ABI_VERSION);
        debug_write("\n");
    } else {
        debug_write("AW_RING3_FAIL reason=abi\n");
    }

    // The syscall entry ran on the user GS and had to swapgs to reach the kernel
    // area. The handler recorded gs:[0]; it proves the swap only if it read the
    // kernel magic (and not the user area's null).
    let observed = OBSERVED_KERNEL_GS.load(Ordering::Relaxed);
    debug_write("AW_SWAPGS_GS observed=");
    debug_write_hex_u64(observed);
    debug_write(" expected=");
    debug_write_hex_u64(KERNEL_GS_MAGIC);
    debug_write("\n");
    if observed == KERNEL_GS_MAGIC {
        debug_write("AW_SWAPGS_PROOF_OK\n");
    } else {
        debug_write("AW_SWAPGS_FAIL\n");
    }

    // Clear the swapgs shadow now the proof is done; the live GS is the kernel
    // area and install_bootstrap_per_cpu is about to set the real per-CPU base.
    // SAFETY: CPL0.
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, 0) };
}

// ---- Ring 3 preemption (dossier sections 8-9, roadmap P0) ------------------
//
// The syscall proof drops to CPL3 with interrupts masked, so the only way back is
// a syscall. This proves the asynchronous half: a user thread that never makes a
// syscall - it just spins incrementing a counter with interrupts enabled - is
// preempted by the timer, and the kernel takes control back on its own.
//
// It reuses the timer-driven switcher in `scheduler`: slot 0 is this kernel path,
// slot 1 is the user thread, whose initial CPL3 interrupt frame is built here so
// the timer ISR's `iretq` epilogue drops straight to CPL3. The ISR's conditional
// swapgs keeps the per-CPU GS correct across the CPL3<->CPL0 boundary, so this is
// also where swapgs stops being a static proof and starts being load-bearing.

const R3P_DATA_VA: u64 = 0x2_1000_0000;
const R3P_CODE_VA: u64 = 0x2_1000_1000;
const R3P_STACK_VA: u64 = 0x2_1000_2000;
const R3P_STACK_TOP: u64 = R3P_STACK_VA + 0x1000;

/// Timer-driven switches for the user run: one to enter the user thread, then
/// several while it spins, before control returns to the kernel.
const R3P_LIMIT: u32 = 6;

/// User spinner, position dependent: `mov rax, R3P_DATA_VA; inc qword [rax];
/// jmp back`. It touches only the counter, never the stack, and never returns.
const R3P_USER_CODE: [u8; 15] = [
    0x48, 0xb8, 0x00, 0x00, 0x00, 0x10, 0x02, 0x00, 0x00, 0x00, // mov rax, 0x2_1000_0000
    0x48, 0xff, 0x00, // inc qword ptr [rax]
    0xeb, 0xfb, // jmp -5 (back to the inc)
];

/// Slot 1's initial CPL3 interrupt frame: 15 zeroed GP registers then the CPU
/// interrupt frame (rip, cs, rflags, rsp, ss), laid out exactly as the timer ISR
/// pushes and pops one.
#[repr(C, align(16))]
struct UserFrame([u64; 20]);
static mut R3P_USER_FRAME: UserFrame = UserFrame([0; 20]);

/// Prove a user thread is preempted by the timer.
///
/// # Safety
/// CPL0 on the bootstrap processor, after the per-CPU block is installed and the
/// APIC timer gate exists, before any application processor is online. Returns
/// with the timer masked and interrupts disabled.
pub unsafe fn prove_ring3_preemption() {
    debug_write("AW_RING3_PREEMPT_BEGIN\n");

    // Map the spinner's code (user, executable, read-only), its counter page and
    // a user stack (both user, writable, non-executable).
    let Some(code_frame) = frame_allocator::allocate() else {
        debug_write("AW_RING3_PREEMPT_FAIL reason=no_code_frame\n");
        return;
    };
    // SAFETY: write the routine through the frame's identity address.
    unsafe {
        core::ptr::copy_nonoverlapping(
            R3P_USER_CODE.as_ptr(),
            code_frame as *mut u8,
            R3P_USER_CODE.len(),
        );
    }
    let Some(data_frame) = frame_allocator::allocate() else {
        debug_write("AW_RING3_PREEMPT_FAIL reason=no_data_frame\n");
        return;
    };
    // SAFETY: zero the counter through the frame's identity address.
    unsafe { (data_frame as *mut u64).write(0) };
    let Some(stack_frame) = frame_allocator::allocate() else {
        debug_write("AW_RING3_PREEMPT_FAIL reason=no_stack_frame\n");
        return;
    };

    let rw = PageTableFlags::USER_ACCESSIBLE
        .union(PageTableFlags::WRITABLE)
        .union(PageTableFlags::NO_EXECUTE);
    // SAFETY: CPL0; these VAs are unused and the frames were just allocated.
    let mapped = unsafe {
        page_mapper::map_page(R3P_CODE_VA, code_frame, PageTableFlags::USER_ACCESSIBLE).is_ok()
            && page_mapper::map_page(R3P_DATA_VA, data_frame, rw).is_ok()
            && page_mapper::map_page(R3P_STACK_VA, stack_frame, rw).is_ok()
    };
    if !mapped {
        debug_write("AW_RING3_PREEMPT_FAIL reason=map\n");
        return;
    }

    // Build slot 1's CPL3 entry frame.
    let frame = core::ptr::addr_of_mut!(R3P_USER_FRAME) as *mut u64;
    // SAFETY: R3P_USER_FRAME holds 20 qwords; fill the whole frame.
    unsafe {
        for offset in 0..15 {
            frame.add(offset).write(0); // r15..rax
        }
        frame.add(15).write(R3P_CODE_VA); // rip
        frame.add(16).write(USER_CODE_SELECTOR); // cs (RPL 3)
        frame.add(17).write(0x202); // rflags: IF set, reserved bit 1
        frame.add(18).write(R3P_STACK_TOP); // rsp
        frame.add(19).write(USER_DATA_SELECTOR); // ss (RPL 3)
    }
    crate::scheduler::set_user_slot_frame(frame as u64);

    // Arm the CPL3<->CPL0 GS convention: at CPL0 the live GS is the kernel per-CPU
    // block and IA32_KERNEL_GS_BASE shadows the user base, so the first drop to
    // CPL3 swaps the user base in and every syscall-less timer entry from CPL3
    // swaps the kernel base back for the per-CPU counter.
    let kernel_gs = crate::percpu::by_index(0)
        .map(|block| core::ptr::from_ref(block) as u64)
        .unwrap_or(0);
    let user_gs = core::ptr::addr_of!(USER_GS_AREA) as u64;
    // SAFETY: CPL0; RSP0 must point at a kernel stack for the from-CPL3 interrupt,
    // and the shadow GS base is armed for swapgs.
    unsafe {
        interrupts::set_bootstrap_rsp0(ring0_stack_top());
        wrmsr(IA32_KERNEL_GS_BASE, user_gs);
    }

    // SAFETY: CPL0, per-CPU installed, timer gate present, no AP online yet.
    let (switches, saw_user) = unsafe { crate::scheduler::run_user_preemption(R3P_LIMIT) };

    // SAFETY: the user thread advanced its counter through the identity mapping of
    // the page it wrote at CPL3.
    let count = unsafe { (data_frame as *const u64).read() };

    // Restore the kernel GS shadow now the user run is over.
    // SAFETY: CPL0; the live GS is the kernel per-CPU block again.
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, kernel_gs) };

    debug_write("AW_RING3_PREEMPT_STATE switches=");
    debug_write_u64(u64::from(switches));
    debug_write(" user_count=");
    debug_write_u64(count);
    debug_write("\n");

    if switches == R3P_LIMIT && saw_user && count > 0 {
        debug_write("AW_RING3_PREEMPT_PROOF_OK\n");
    } else {
        debug_write("AW_RING3_PREEMPT_FAIL reason=not_preempted\n");
    }
}

// ---- Userland ELF loader (dossier section 12, roadmap P0 step 7) ------------
//
// Ring 3 and the syscall ABI proved the privilege boundary with a hand-placed
// routine; FAT16 proved a real file read. This joins them: it reads an ELF from
// the disk, maps its PT_LOAD segments as user pages, and runs it at CPL3. The
// loaded program reports a known value through SYS_REPORT and exits; control
// returning to the kernel with that value proves the program on disk actually ran.

/// 8.3 directory name of the userland program on the FAT16 disk.
const USER_PROGRAM_NAME: &[u8; 11] = b"USERPROGELF";
/// The value the loaded program reports, so the proof knows it was that program.
const EXPECTED_REPORT: u64 = 0xc0de;
/// User stack for the loaded program, above the identity map and other user VAs.
const LOADER_STACK_VA: u64 = 0x4_1000_0000;
const LOADER_STACK_TOP: u64 = LOADER_STACK_VA + 0x1000;

fn elf_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}
fn elf_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}
fn elf_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

/// Map one PT_LOAD segment: a frame per page, the file bytes copied in through the
/// frame's identity address and the tail zeroed, mapped at `p_vaddr` with user
/// access and the segment's execute/write permission.
fn map_segment(
    image: &[u8],
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    exec: bool,
    write: bool,
) -> bool {
    if p_vaddr & 0xfff != 0 {
        return false; // this minimal loader only maps page-aligned segments
    }
    let mut flags = PageTableFlags::USER_ACCESSIBLE;
    if write {
        flags = flags.union(PageTableFlags::WRITABLE);
    }
    if !exec {
        flags = flags.union(PageTableFlags::NO_EXECUTE);
    }
    let pages = (p_memsz as usize).div_ceil(4096);
    for page in 0..pages {
        let Some(frame) = frame_allocator::allocate() else {
            return false;
        };
        // SAFETY: the frame's identity address is kernel-writable; fill the page.
        unsafe {
            let dst = frame as *mut u8;
            for i in 0..4096usize {
                let in_segment = page * 4096 + i;
                let byte = if in_segment < p_filesz as usize {
                    *image.get(p_offset as usize + in_segment).unwrap_or(&0)
                } else {
                    0
                };
                dst.add(i).write(byte);
            }
        }
        // SAFETY: a fresh high user VA, backed by the frame just allocated.
        if unsafe { page_mapper::map_page(p_vaddr + (page as u64) * 4096, frame, flags) }.is_err() {
            return false;
        }
    }
    true
}

/// Validate an ELF64 image and map every PT_LOAD segment as user pages. Returns
/// `(entry, base)` where `base` is the lowest PT_LOAD virtual address.
///
/// # Safety
/// CPL0; maps into the live kernel page tables.
unsafe fn load_elf_image(image: &[u8]) -> Option<(u64, u64)> {
    let magic_ok =
        image.len() >= 64 && &image[0..4] == b"\x7fELF" && image[4] == 2 && image[5] == 1;
    if !magic_ok || elf_u16(image, 18) != Some(0x3e) {
        return None;
    }
    let entry = elf_u64(image, 24)?;
    let phoff = elf_u64(image, 32)? as usize;
    let phentsize = elf_u16(image, 54)? as usize;
    let phnum = elf_u16(image, 56)? as usize;
    if phentsize < 56 || phnum == 0 {
        return None;
    }
    let mut base = u64::MAX;
    let mut loaded = 0;
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if elf_u32(image, ph) != Some(1) {
            continue; // PT_LOAD only
        }
        let p_flags = elf_u32(image, ph + 4)?;
        let p_offset = elf_u64(image, ph + 8)?;
        let p_vaddr = elf_u64(image, ph + 16)?;
        let p_filesz = elf_u64(image, ph + 32)?;
        let p_memsz = elf_u64(image, ph + 40)?;
        if !map_segment(
            image,
            p_offset,
            p_vaddr,
            p_filesz,
            p_memsz,
            p_flags & 1 != 0,
            p_flags & 2 != 0,
        ) {
            return None;
        }
        base = base.min(p_vaddr);
        loaded += 1;
    }
    if loaded == 0 {
        return None;
    }
    Some((entry, base))
}

/// Load a userland ELF from the FAT16 disk and run it at CPL3.
///
/// # Safety
/// CPL0 on the bootstrap processor, after paging, the heap and the first Ring 3
/// proof are up, with the virtio-block device brought up. Interrupts masked.
pub unsafe fn prove_user_loader(device: &BlkDevice) {
    debug_write("AW_USER_LOADER_BEGIN\n");
    let Some(image) = fat16::load_file(device, USER_PROGRAM_NAME) else {
        debug_write("AW_USER_LOADER_UNAVAILABLE reason=no_file\n");
        return;
    };

    // SAFETY: CPL0; maps the image into the live kernel page tables.
    let Some((entry, _base)) = (unsafe { load_elf_image(&image) }) else {
        debug_write("AW_USER_LOADER_FAIL reason=bad_elf\n");
        return;
    };

    let Some(stack_frame) = frame_allocator::allocate() else {
        debug_write("AW_USER_LOADER_FAIL reason=no_stack\n");
        return;
    };
    let stack_flags = PageTableFlags::USER_ACCESSIBLE
        .union(PageTableFlags::WRITABLE)
        .union(PageTableFlags::NO_EXECUTE);
    // SAFETY: CPL0; the stack VA is unused and the frame was just allocated.
    if unsafe { page_mapper::map_page(LOADER_STACK_VA, stack_frame, stack_flags) }.is_err() {
        debug_write("AW_USER_LOADER_FAIL reason=map_stack\n");
        return;
    }

    debug_write("AW_USER_LOADER_MAP_OK entry=");
    debug_write_hex_u64(entry);
    debug_write("\n");

    REPORTED_SEEN.store(false, Ordering::Relaxed);
    SYSCALL_EXIT.store(0, Ordering::Relaxed);
    let kernel_gs = crate::percpu::by_index(0)
        .map(|block| core::ptr::from_ref(block) as u64)
        .unwrap_or(0);
    // SAFETY: CPL0; set rsp0, the syscall MSRs and the kernel GS shadow before CPL3.
    unsafe {
        interrupts::set_bootstrap_rsp0(ring0_stack_top());
        KERNEL_SYSCALL_RSP.store(ring0_stack_top(), Ordering::Relaxed);
        enable_syscall();
        wrmsr(IA32_KERNEL_GS_BASE, kernel_gs);
    }
    // SAFETY: user pages mapped, MSRs and rsp0 set, interrupts masked.
    unsafe { aw_enter_ring3(entry, LOADER_STACK_TOP) };

    let seen = REPORTED_SEEN.load(Ordering::Relaxed);
    let value = REPORTED_VALUE.load(Ordering::Relaxed);
    debug_write("AW_USER_LOADER_REPORT seen=");
    debug_write_u64(u64::from(seen));
    debug_write(" value=");
    debug_write_hex_u64(value);
    debug_write("\n");
    if seen && value == EXPECTED_REPORT {
        debug_write("AW_USER_LOADER_PROOF_OK\n");
    } else {
        debug_write("AW_USER_LOADER_FAIL reason=no_report\n");
    }

    // SAFETY: CPL0; clear the GS shadow now the program has exited.
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, 0) };
}

// ---- init: preemptively schedule two userland programs (roadmap P0 step 7) ---
//
// The loader ran one program to completion. This runs two at once: two userland
// spinners are loaded from disk, each incrementing a counter in its own page, and
// the timer preempts back and forth between them (reusing the Ring 3 preemption
// switcher). Both counters advancing proves the kernel schedules more than one
// userland program - the seed of an init/service manager. They share one address
// space for now; per-process isolation is a later step.

const INIT_A_NAME: &[u8; 11] = b"USERA   ELF";
const INIT_B_NAME: &[u8; 11] = b"USERB   ELF";
/// Timer-driven switches for the init run: enough for both programs to run several
/// times in strict A/B rotation before control returns to the kernel.
const INIT_LIMIT: u32 = 14;

static mut INIT_FRAME_A: UserFrame = UserFrame([0; 20]);
static mut INIT_FRAME_B: UserFrame = UserFrame([0; 20]);

// A separate kernel stack per user program, so each takes its CPL3 interrupts on
// its own stack (the TSS RSP0 is switched to the running one) instead of both
// colliding on one and clobbering each other's saved frame.
static mut INIT_KSTACK_A: KernelStack = KernelStack([0; 16 * 1024]);
static mut INIT_KSTACK_B: KernelStack = KernelStack([0; 16 * 1024]);

fn init_kstack_top(kstack: *const KernelStack) -> u64 {
    (kstack as u64 + 16 * 1024) & !0xf_u64
}

/// Fill a 20-qword buffer as an initial CPL3 interrupt frame the timer ISR's
/// pop/iretq epilogue can start: 15 zero GP registers, then rip/cs/rflags/rsp/ss.
fn build_cpl3_frame(frame: *mut u64, entry: u64, rsp: u64) {
    // SAFETY: the caller passes a live 20-qword buffer.
    unsafe {
        for i in 0..15 {
            frame.add(i).write(0);
        }
        frame.add(15).write(entry);
        frame.add(16).write(USER_CODE_SELECTOR);
        frame.add(17).write(0x202); // IF set, reserved bit 1
        frame.add(18).write(rsp);
        frame.add(19).write(USER_DATA_SELECTOR);
    }
}

/// Load two userland programs and let the timer preemptively schedule both at
/// CPL3.
///
/// # Safety
/// CPL0 on the bootstrap processor, after paging/heap/Ring 3 are up and the
/// virtio-block device is present, before any AP is online. Interrupts masked.
pub unsafe fn prove_user_init(device: &BlkDevice) {
    debug_write("AW_USER_INIT_BEGIN\n");
    let (Some(image_a), Some(image_b)) = (
        fat16::load_file(device, INIT_A_NAME),
        fat16::load_file(device, INIT_B_NAME),
    ) else {
        debug_write("AW_USER_INIT_UNAVAILABLE reason=no_files\n");
        return;
    };

    // SAFETY: CPL0; map both images into the live kernel page tables.
    let (Some((entry_a, base_a)), Some((entry_b, base_b))) =
        (unsafe { load_elf_image(&image_a) }, unsafe {
            load_elf_image(&image_b)
        })
    else {
        debug_write("AW_USER_INIT_FAIL reason=bad_elf\n");
        return;
    };

    // A read-write work page per program at base+0x1000: the spinner's counter is
    // at its start, and base+0x2000 serves as its (never-touched) stack top.
    let work_flags = PageTableFlags::USER_ACCESSIBLE
        .union(PageTableFlags::WRITABLE)
        .union(PageTableFlags::NO_EXECUTE);
    let (Some(work_a), Some(work_b)) = (frame_allocator::allocate(), frame_allocator::allocate())
    else {
        debug_write("AW_USER_INIT_FAIL reason=no_frame\n");
        return;
    };
    // SAFETY: zero both counters through their frames' identity addresses.
    unsafe {
        (work_a as *mut u64).write(0);
        (work_b as *mut u64).write(0);
    }
    // SAFETY: CPL0; fresh work pages one page above each program's image.
    let mapped = unsafe {
        page_mapper::map_page(base_a + 0x1000, work_a, work_flags).is_ok()
            && page_mapper::map_page(base_b + 0x1000, work_b, work_flags).is_ok()
    };
    if !mapped {
        debug_write("AW_USER_INIT_FAIL reason=map_work\n");
        return;
    }

    build_cpl3_frame(
        core::ptr::addr_of_mut!(INIT_FRAME_A) as *mut u64,
        entry_a,
        base_a + 0x2000,
    );
    build_cpl3_frame(
        core::ptr::addr_of_mut!(INIT_FRAME_B) as *mut u64,
        entry_b,
        base_b + 0x2000,
    );
    crate::scheduler::set_slot_frame(1, core::ptr::addr_of!(INIT_FRAME_A) as u64);
    crate::scheduler::set_slot_frame(2, core::ptr::addr_of!(INIT_FRAME_B) as u64);
    // Each user slot takes its interrupts on its own kernel stack.
    crate::scheduler::set_slot_kstack(1, init_kstack_top(core::ptr::addr_of!(INIT_KSTACK_A)));
    crate::scheduler::set_slot_kstack(2, init_kstack_top(core::ptr::addr_of!(INIT_KSTACK_B)));

    // Arm the swapgs convention as in the single-program preemption proof: the
    // live GS is the kernel per-CPU block and the shadow holds the user base, so
    // each timer entry from CPL3 swaps the kernel base in for the per-CPU counter.
    let user_gs = core::ptr::addr_of!(USER_GS_AREA) as u64;
    // SAFETY: CPL0; RSP0 for the from-CPL3 interrupts and the swapgs shadow.
    unsafe {
        interrupts::set_bootstrap_rsp0(ring0_stack_top());
        wrmsr(IA32_KERNEL_GS_BASE, user_gs);
    }

    // SAFETY: both slots hold valid CPL3 frames, the swapgs shadow is armed, no AP
    // is online, and the timer gate is installed.
    let (switches, saw_user) =
        unsafe { crate::scheduler::run_preemption_over(&[1, 2], INIT_LIMIT) };

    // SAFETY: both counters were advanced at CPL3 through their identity-mapped pages.
    let count_a = unsafe { (work_a as *const u64).read() };
    let count_b = unsafe { (work_b as *const u64).read() };
    // SAFETY: CPL0; clear the GS shadow now the programs are descheduled.
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, 0) };

    debug_write("AW_USER_INIT_STATE switches=");
    debug_write_u64(u64::from(switches));
    debug_write(" a=");
    debug_write_u64(count_a);
    debug_write(" b=");
    debug_write_u64(count_b);
    debug_write("\n");
    // Both must have run, and run *comparably*: correct A/B alternation gives each
    // program a similar number of equal-length timeslices, so the counts stay close.
    // A lopsided ratio would mean one program was starved - the shape of a broken
    // switch that let only one keep running - so the proof rejects it.
    let low = count_a.min(count_b);
    let high = count_a.max(count_b);
    let balanced = low > 0 && high <= low.saturating_mul(3);
    if switches == INIT_LIMIT && saw_user && balanced {
        debug_write("AW_USER_INIT_PROOF_OK\n");
    } else {
        debug_write("AW_USER_INIT_FAIL reason=unbalanced\n");
    }
}

// ---- IPC and the handle/object model (roadmap Phase 2) ----------------------
//
// Two programs from disk, preemptively scheduled at CPL3 as in the init proof,
// talk through a kernel channel they can only reach through handles. A holds the
// send end, B the receive end. Before sending, A tries five things the model must
// refuse: a handle value that only exists in B's table, its own handle with a
// forged generation, receiving on its send-only end, sending from a kernel
// address, and using a handle after closing it. Then A sends IPC_MESSAGES
// numbered messages and B checks each one arrives once, in order, intact.

const IPC_A_NAME: &[u8; 11] = b"USERIPCAELF";
const IPC_B_NAME: &[u8; 11] = b"USERIPCBELF";
const IPC_MESSAGES: u64 = 32;
const IPC_LIMIT: u32 = 60;
// Work-page layout, shared with scripts/make-user-elf.py.
const W_HANDLE: usize = 0x08;
const W_FOREIGN: usize = 0x10;
const W_FORGED: usize = 0x18;
const W_TEMP: usize = 0x20;
const W_RESULTS: usize = 0x40;
const W_COUNT: usize = 0x80;
const W_SUM: usize = 0x90;
const W_BAD_ORDER: usize = 0x98;

static mut IPC_FRAME_A: UserFrame = UserFrame([0; 20]);
static mut IPC_FRAME_B: UserFrame = UserFrame([0; 20]);
static mut IPC_KSTACK_A: KernelStack = KernelStack([0; 16 * 1024]);
static mut IPC_KSTACK_B: KernelStack = KernelStack([0; 16 * 1024]);

fn work_read(frame: u64, offset: usize) -> u64 {
    // SAFETY: `frame` is an identity-mapped work page the kernel allocated.
    unsafe { ((frame as usize + offset) as *const u64).read_volatile() }
}
fn work_write(frame: u64, offset: usize, value: u64) {
    // SAFETY: as above.
    unsafe { ((frame as usize + offset) as *mut u64).write_volatile(value) }
}

/// Run the IPC programs and check both the traffic and the refusals.
///
/// # Safety
/// Same contract as [`prove_user_init`], which it follows.
pub unsafe fn prove_user_ipc(device: &BlkDevice) {
    use crate::ipc;
    debug_write("AW_IPC_BEGIN\n");
    let (Some(image_a), Some(image_b)) = (
        fat16::load_file(device, IPC_A_NAME),
        fat16::load_file(device, IPC_B_NAME),
    ) else {
        debug_write("AW_IPC_UNAVAILABLE reason=no_files\n");
        return;
    };
    // SAFETY: CPL0; map both images into the live kernel page tables.
    let (Some((entry_a, base_a)), Some((entry_b, base_b))) =
        (unsafe { load_elf_image(&image_a) }, unsafe {
            load_elf_image(&image_b)
        })
    else {
        debug_write("AW_IPC_FAIL reason=bad_elf\n");
        return;
    };
    let work_flags = PageTableFlags::USER_ACCESSIBLE
        .union(PageTableFlags::WRITABLE)
        .union(PageTableFlags::NO_EXECUTE);
    let (Some(work_a), Some(work_b)) = (frame_allocator::allocate(), frame_allocator::allocate())
    else {
        debug_write("AW_IPC_FAIL reason=no_frame\n");
        return;
    };
    // SAFETY: zero both work pages through their identity addresses, then map them.
    unsafe {
        core::ptr::write_bytes(work_a as *mut u8, 0, 4096);
        core::ptr::write_bytes(work_b as *mut u8, 0, 4096);
    }
    let mapped = unsafe {
        page_mapper::map_page(base_a + 0x1000, work_a, work_flags).is_ok()
            && page_mapper::map_page(base_b + 0x1000, work_b, work_flags).is_ok()
    };
    if !mapped {
        debug_write("AW_IPC_FAIL reason=map_work\n");
        return;
    }

    // Processes, one channel, and the handles each side is born with.
    ipc::reset();
    let objects_ok = ipc::create_process(0, 1, base_a, base_a + 0x2000)
        && ipc::create_process(1, 2, base_b, base_b + 0x2000);
    let channel = ipc::create_channel();
    let (Some(channel), true) = (channel, objects_ok) else {
        debug_write("AW_IPC_FAIL reason=setup\n");
        return;
    };
    let (Some(send_a), Some(temp_a), Some(recv_b)) = (
        ipc::grant(0, channel, ipc::RIGHT_SEND),
        ipc::grant(0, channel, ipc::RIGHT_SEND),
        ipc::grant(1, channel, ipc::RIGHT_RECV),
    ) else {
        debug_write("AW_IPC_FAIL reason=grant\n");
        return;
    };
    work_write(work_a, W_HANDLE, send_a);
    work_write(work_a, W_FOREIGN, recv_b); // valid only in B's table
    work_write(work_a, W_FORGED, send_a ^ (0x7f << 16)); // right slot, wrong generation
    work_write(work_a, W_TEMP, temp_a);
    work_write(work_b, W_HANDLE, recv_b);

    build_cpl3_frame(
        core::ptr::addr_of_mut!(IPC_FRAME_A) as *mut u64,
        entry_a,
        base_a + 0x2000,
    );
    build_cpl3_frame(
        core::ptr::addr_of_mut!(IPC_FRAME_B) as *mut u64,
        entry_b,
        base_b + 0x2000,
    );
    crate::scheduler::set_slot_frame(1, core::ptr::addr_of!(IPC_FRAME_A) as u64);
    crate::scheduler::set_slot_frame(2, core::ptr::addr_of!(IPC_FRAME_B) as u64);
    crate::scheduler::set_slot_kstack(1, init_kstack_top(core::ptr::addr_of!(IPC_KSTACK_A)));
    crate::scheduler::set_slot_kstack(2, init_kstack_top(core::ptr::addr_of!(IPC_KSTACK_B)));

    let user_gs = core::ptr::addr_of!(USER_GS_AREA) as u64;
    // SAFETY: CPL0; syscall MSRs and stack for the programs' syscalls, RSP0 and the
    // swapgs shadow for their interrupts (same convention as the init proof).
    unsafe {
        interrupts::set_bootstrap_rsp0(ring0_stack_top());
        KERNEL_SYSCALL_RSP.store(ring0_stack_top(), Ordering::Relaxed);
        SYSCALL_EXIT.store(0, Ordering::Relaxed);
        enable_syscall();
        wrmsr(IA32_KERNEL_GS_BASE, user_gs);
    }
    // SAFETY: both slots hold valid CPL3 frames, no AP online, timer gate present.
    let (switches, saw_user) = unsafe { crate::scheduler::run_preemption_over(&[1, 2], IPC_LIMIT) };
    // SAFETY: CPL0; clear the GS shadow now the programs are descheduled.
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, 0) };

    // The five refusals A recorded, in order.
    let results = [0, 1, 2, 3, 4].map(|i| work_read(work_a, W_RESULTS + i * 8));
    let expected = [
        ipc::E_BAD_HANDLE,
        ipc::E_BAD_HANDLE,
        ipc::E_ACCESS,
        ipc::E_FAULT,
        ipc::E_BAD_HANDLE,
    ];
    let refusals_ok = results == expected;
    let sent = work_read(work_a, W_COUNT);
    let received = work_read(work_b, W_COUNT);
    let sum = work_read(work_b, W_SUM);
    let bad = work_read(work_b, W_BAD_ORDER);
    let delivered = ipc::DELIVERED.load(Ordering::Relaxed);
    // Closing the spare handle dropped one reference; the channel must still live.
    let channel_alive = ipc::live_objects() == 1;

    debug_write("AW_IPC_STATE switches=");
    debug_write_u64(u64::from(switches));
    debug_write(" sent=");
    debug_write_u64(sent);
    debug_write(" received=");
    debug_write_u64(received);
    debug_write(" delivered=");
    debug_write_u64(delivered);
    debug_write(" sum=");
    debug_write_u64(sum);
    debug_write(" bad=");
    debug_write_u64(bad);
    debug_write("\n");
    debug_write("AW_HANDLE_REFUSALS");
    for (name, value) in ["foreign", "forged", "rights", "kernel_ptr", "closed"]
        .iter()
        .zip(results)
    {
        debug_write(" ");
        debug_write(name);
        debug_write("=");
        debug_write_hex_u64(value);
    }
    debug_write("\n");

    let traffic_ok = sent == IPC_MESSAGES
        && received == IPC_MESSAGES
        && delivered == IPC_MESSAGES
        && sum == IPC_MESSAGES * (IPC_MESSAGES + 1) / 2
        && bad == 0;
    if refusals_ok && channel_alive {
        debug_write("AW_HANDLE_SECURITY_PROOF_OK\n");
    } else {
        debug_write("AW_HANDLE_SECURITY_FAIL\n");
    }
    if switches == IPC_LIMIT && saw_user && traffic_ok {
        debug_write("AW_IPC_PROOF_OK messages=");
        debug_write_u64(received);
        debug_write("\n");
    } else {
        debug_write("AW_IPC_FAIL reason=traffic\n");
    }
}
