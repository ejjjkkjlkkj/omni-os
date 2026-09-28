//! GDT, TSS/IST and IDT bring-up for the native x86-64 kernel.
//!
//! A full 256-entry IDT is installed. CPU exception vectors 0-31 point at
//! dedicated stubs; the remaining vectors start out not-present so an
//! unexpected interrupt raises `#GP` rather than dispatching through a stale
//! gate. Vector 8 (#DF) uses IST1 so a corrupted current stack cannot prevent
//! double-fault diagnostics. External interrupt vectors (APIC timer, later
//! device IRQs) are wired in after `install()` through
//! [`install_interrupt_gate`], which reuses the same audited descriptor
//! encoder as the exception gates.

use aw_x86_interrupts::{
    DescriptorTablePointer, GateType, GdtEntry, IdtEntry, IdtEntryError, PrivilegeLevel,
    SegmentSelector, exception_pushes_error_code,
};
use core::arch::{asm, naked_asm};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::{debug_write, debug_write_hex_u64, halt_forever};

const CODE_DESCRIPTOR: u64 = GdtEntry::code64(PrivilegeLevel::Ring0).raw();
const DATA_DESCRIPTOR: u64 = GdtEntry::data64(PrivilegeLevel::Ring0).raw();
const USER_DATA_DESCRIPTOR: u64 = GdtEntry::data64(PrivilegeLevel::Ring3).raw();
const USER_CODE_DESCRIPTOR: u64 = GdtEntry::code64(PrivilegeLevel::Ring3).raw();

const CODE_SELECTOR_INDEX: u16 = 1;
const DATA_SELECTOR_INDEX: u16 = 2;
const TSS_SELECTOR_INDEX: u16 = 3;
// User segments follow the TSS descriptor (which occupies indices 3 and 4), so
// the TSS selector stays 0x18 and every existing GDT/TSS/SMP proof is untouched.
// The order - data at 5, code at 6 - is what a later `sysret` needs: with
// `IA32_STAR[63:48] = 0x20`, `sysret` derives user SS from 0x20+8 = 0x28 and
// user CS from 0x20+16 = 0x30.
const USER_DATA_SELECTOR_INDEX: u16 = 5;
const USER_CODE_SELECTOR_INDEX: u16 = 6;
const CODE_SELECTOR_RAW: u16 = CODE_SELECTOR_INDEX << 3;
const DATA_SELECTOR_RAW: u16 = DATA_SELECTOR_INDEX << 3;
const TSS_SELECTOR_RAW: u16 = TSS_SELECTOR_INDEX << 3;
/// Ring 3 selectors carry RPL 3 (the low two bits).
pub(crate) const USER_DATA_SELECTOR_RAW: u16 = (USER_DATA_SELECTOR_INDEX << 3) | 3;
pub(crate) const USER_CODE_SELECTOR_RAW: u16 = (USER_CODE_SELECTOR_INDEX << 3) | 3;
/// Number of 8-byte slots in the GDT: null, kernel code/data, the TSS
/// descriptor (two slots) and the user data/code segments.
const GDT_ENTRY_COUNT: usize = 7;

/// Number of CPU-reserved exception vectors that get a dedicated stub.
const EXCEPTION_IDT_ENTRY_COUNT: usize = 32;
/// Full architectural IDT length: 32 exception vectors plus 224 vectors
/// available for external/software interrupts.
const IDT_ENTRY_COUNT: usize = aw_x86_interrupts::IDT_ENTRY_COUNT;
const DOUBLE_FAULT_VECTOR: usize = 8;
#[cfg(any(feature = "double-fault-smoke-test", feature = "ap-double-fault-smoke-test"))]
const GENERAL_PROTECTION_VECTOR: usize = 13;
const DOUBLE_FAULT_IST_INDEX: u8 = 1;
const DOUBLE_FAULT_IST_STACK_SIZE: usize = 16 * 1024;

const fn selector(index: u16) -> SegmentSelector {
    match SegmentSelector::new(index, PrivilegeLevel::Ring0) {
        Some(value) => value,
        None => panic!("GDT index does not fit in a segment selector"),
    }
}

const CODE_SELECTOR: SegmentSelector = selector(CODE_SELECTOR_INDEX);

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct TaskStateSegment {
    reserved0: u32,
    rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    reserved1: u64,
    ist1: u64,
    ist2: u64,
    ist3: u64,
    ist4: u64,
    ist5: u64,
    ist6: u64,
    ist7: u64,
    reserved2: u64,
    reserved3: u16,
    io_map_base: u16,
}

impl TaskStateSegment {
    const EMPTY: Self = Self {
        reserved0: 0,
        rsp0: 0,
        rsp1: 0,
        rsp2: 0,
        reserved1: 0,
        ist1: 0,
        ist2: 0,
        ist3: 0,
        ist4: 0,
        ist5: 0,
        ist6: 0,
        ist7: 0,
        reserved2: 0,
        reserved3: 0,
        io_map_base: core::mem::size_of::<Self>() as u16,
    };

    const fn with_ist1(stack_top: u64) -> Self {
        Self {
            ist1: stack_top,
            ..Self::EMPTY
        }
    }
}

const _: () = assert!(core::mem::size_of::<TaskStateSegment>() == 104);

/// The #DF emergency stack, preceded by a page that the kernel's own page
/// tables deliberately leave unmapped.
///
/// x86 stacks grow downwards, so overflowing this one runs off its low end
/// straight into `guard`. Without the hole, that would quietly scribble over
/// whatever static happened to be laid out below it; with it, the overflow
/// takes a not-present #PF at a known address.
#[repr(C, align(4096))]
struct GuardedIstStack {
    guard: [u8; PAGE_SIZE],
    stack: [u8; DOUBLE_FAULT_IST_STACK_SIZE],
}

const PAGE_SIZE: usize = 4096;

const _: () = assert!(DOUBLE_FAULT_IST_STACK_SIZE.is_multiple_of(PAGE_SIZE));

#[used]
#[unsafe(link_section = ".data.gdt")]
static mut GDT: [u64; GDT_ENTRY_COUNT] = [
    0,
    CODE_DESCRIPTOR,
    DATA_DESCRIPTOR,
    0,
    0,
    USER_DATA_DESCRIPTOR,
    USER_CODE_DESCRIPTOR,
];

#[used]
#[unsafe(link_section = ".data.tss")]
static mut TSS: TaskStateSegment = TaskStateSegment::EMPTY;

#[used]
#[unsafe(link_section = ".data.ist")]
static mut DOUBLE_FAULT_IST_STACK: GuardedIstStack = GuardedIstStack {
    guard: [0; PAGE_SIZE],
    stack: [0; DOUBLE_FAULT_IST_STACK_SIZE],
};

/// Address of the unmapped page below the #DF emergency stack.
#[must_use]
pub(crate) fn double_fault_guard_page() -> u64 {
    core::ptr::addr_of!(DOUBLE_FAULT_IST_STACK) as u64
}

/// Application processors this kernel is willing to bring up, plus the
/// bootstrap processor. A fixed bound keeps every per-CPU table in `.bss` with
/// no allocator in the path; a machine with more CPUs leaves the rest parked.
pub(crate) const MAX_CPUS: usize = 8;

/// Per-AP #DF stack. Smaller than the bootstrap processor's, and its guard page
/// is *not* unmapped: the kernel's page tables were built before these existed,
/// so an AP stack overflow is currently silent. Tracked as a known limitation.
const AP_IST_STACK_SIZE: usize = 8 * 1024;

#[repr(C, align(4096))]
struct ApIstStack {
    guard: [u8; PAGE_SIZE],
    stack: [u8; AP_IST_STACK_SIZE],
}

static mut AP_GDTS: [[u64; GDT_ENTRY_COUNT]; MAX_CPUS] = [[0; GDT_ENTRY_COUNT]; MAX_CPUS];
static mut AP_TSSES: [TaskStateSegment; MAX_CPUS] = [TaskStateSegment::EMPTY; MAX_CPUS];
static mut AP_IST_STACKS: [ApIstStack; MAX_CPUS] = [const {
    ApIstStack {
        guard: [0; PAGE_SIZE],
        stack: [0; AP_IST_STACK_SIZE],
    }
}; MAX_CPUS];

/// What one application processor loaded, read back from the CPU itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ApTables {
    pub task_register: u16,
    pub gdt_base: u64,
    pub tss_base: u64,
    pub ist1_start: u64,
    pub ist1_top: u64,
}

/// Install this application processor's own GDT and TSS, then share the
/// bootstrap processor's IDT.
///
/// The IDT is shared on purpose: interrupt gates hold no per-CPU state. The GDT
/// and TSS cannot be, because the TSS descriptor lives in the GDT and the busy
/// bit `ltr` sets is per-descriptor - two CPUs loading one TSS is a #GP on the
/// second, and an IST shared between CPUs would have them fault onto the same
/// stack.
///
/// # Safety
///
/// Runs once per AP, at CPL0 with interrupts masked, on a CPU whose `cpu` index
/// is unique and below [`MAX_CPUS`].
pub(crate) unsafe fn install_for_ap(cpu: usize) -> Option<ApTables> {
    if cpu == 0 || cpu >= MAX_CPUS {
        return None;
    }

    // SAFETY: `cpu` indexes this AP's own slot, which no other CPU touches.
    let (gdt, tss, ist) = unsafe {
        (
            core::ptr::addr_of_mut!(AP_GDTS)
                .cast::<[u64; GDT_ENTRY_COUNT]>()
                .add(cpu),
            core::ptr::addr_of_mut!(AP_TSSES)
                .cast::<TaskStateSegment>()
                .add(cpu),
            core::ptr::addr_of!(AP_IST_STACKS).cast::<ApIstStack>().add(cpu),
        )
    };

    // SAFETY: taking the address of a field, never reading through it.
    let stack_start = unsafe { core::ptr::addr_of!((*ist).stack) } as *const u8 as u64;
    let ist1_top = (stack_start + AP_IST_STACK_SIZE as u64) & !0xf_u64;

    let (tss_low, tss_high) = encode_tss_descriptor(tss as u64);
    // SAFETY: this CPU's own tables, written before they are loaded.
    unsafe {
        tss.write(TaskStateSegment::with_ist1(ist1_top));
        let entries = gdt.cast::<u64>();
        entries.add(0).write(0);
        entries.add(1).write(CODE_DESCRIPTOR);
        entries.add(2).write(DATA_DESCRIPTOR);
        entries.add(3).write(tss_low);
        entries.add(4).write(tss_high);
        entries.add(5).write(USER_DATA_DESCRIPTOR);
        entries.add(6).write(USER_CODE_DESCRIPTOR);
    }

    let gdt_pointer =
        DescriptorTablePointer::new(gdt as u64, GDT_ENTRY_COUNT, core::mem::size_of::<u64>())
            .ok()?;
    let idt_pointer = DescriptorTablePointer::new(
        core::ptr::addr_of!(IDT) as u64,
        IDT_ENTRY_COUNT,
        core::mem::size_of::<IdtEntry>(),
    )
    .ok()?;

    // SAFETY: CPL0 with interrupts masked; the tables above are fully built and
    // the segment reload makes the visible selectors match the new GDT, which
    // the first `iretq` on this CPU will re-validate.
    unsafe {
        asm!(
            "lgdt [{gdt_pointer}]",
            gdt_pointer = in(reg) &gdt_pointer,
            options(readonly, nostack, preserves_flags),
        );
        reload_segment_registers();
        asm!(
            "lidt [{idt_pointer}]",
            idt_pointer = in(reg) &idt_pointer,
            options(readonly, nostack, preserves_flags),
        );
        asm!(
            "mov ax, 0x18",
            "ltr ax",
            out("ax") _,
            options(nostack, preserves_flags),
        );
    }

    let task_register: u16;
    // SAFETY: reading the task register has no side effects.
    unsafe {
        asm!("str ax", out("ax") task_register, options(nostack, preserves_flags));
    }
    let (code_selector, stack_selector) = current_segments();

    if task_register != TSS_SELECTOR_RAW
        || code_selector != CODE_SELECTOR_RAW
        || stack_selector != DATA_SELECTOR_RAW
    {
        return None;
    }

    Some(ApTables {
        task_register,
        gdt_base: gdt as u64,
        tss_base: tss as u64,
        ist1_start: stack_start,
        ist1_top,
    })
}

/// The bootstrap processor's own tables, for comparison against the APs'.
#[must_use]
pub(crate) fn bootstrap_tables() -> (u64, u64) {
    (
        core::ptr::addr_of!(GDT) as u64,
        core::ptr::addr_of!(TSS) as u64,
    )
}

const fn gate_for_selector(
    handler_address: u64,
    code_selector: SegmentSelector,
    ist_index: u8,
) -> IdtEntry {
    match IdtEntry::new(
        handler_address,
        code_selector,
        GateType::Interrupt,
        PrivilegeLevel::Ring0,
        ist_index,
    ) {
        Ok(entry) => entry,
        Err(_) => panic!("invalid IDT gate"),
    }
}

fn gate(handler: extern "sysv64" fn() -> !, ist_index: u8) -> IdtEntry {
    gate_for_selector(handler as usize as u64, CODE_SELECTOR, ist_index)
}

fn encode_tss_descriptor(base: u64) -> (u64, u64) {
    let limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;

    let low = (limit & 0xffff)
        | ((base & 0x00ff_ffff) << 16)
        | (0x89_u64 << 40)
        | (((limit >> 16) & 0x0f) << 48)
        | (((base >> 24) & 0xff) << 56);
    let high = (base >> 32) & 0xffff_ffff;

    (low, high)
}

macro_rules! assert_error_code_matches {
    ($vector:literal, $has_error_code:literal) => {
        const _: () = assert!(exception_pushes_error_code($vector) == $has_error_code);
    };
}

macro_rules! exception_stub {
    ($name:ident, $vector:literal, no_error_code) => {
        assert_error_code_matches!($vector, false);

        #[unsafe(naked)]
        extern "sysv64" fn $name() -> ! {
            naked_asm!(
                "push 0",
                "push {vector}",
                "jmp {common}",
                vector = const $vector,
                common = sym common_exception_entry,
            )
        }
    };
    ($name:ident, $vector:literal, has_error_code) => {
        assert_error_code_matches!($vector, true);

        #[unsafe(naked)]
        extern "sysv64" fn $name() -> ! {
            naked_asm!(
                "push {vector}",
                "jmp {common}",
                vector = const $vector,
                common = sym common_exception_entry,
            )
        }
    };
}

exception_stub!(exc_stub_00, 0, no_error_code);
exception_stub!(exc_stub_01, 1, no_error_code);
exception_stub!(exc_stub_02, 2, no_error_code);
exception_stub!(exc_stub_03, 3, no_error_code);
exception_stub!(exc_stub_04, 4, no_error_code);
exception_stub!(exc_stub_05, 5, no_error_code);
exception_stub!(exc_stub_06, 6, no_error_code);
exception_stub!(exc_stub_07, 7, no_error_code);
exception_stub!(exc_stub_08, 8, has_error_code);
exception_stub!(exc_stub_09, 9, no_error_code);
exception_stub!(exc_stub_10, 10, has_error_code);
exception_stub!(exc_stub_11, 11, has_error_code);
exception_stub!(exc_stub_12, 12, has_error_code);
exception_stub!(exc_stub_13, 13, has_error_code);
exception_stub!(exc_stub_14, 14, has_error_code);
exception_stub!(exc_stub_15, 15, no_error_code);
exception_stub!(exc_stub_16, 16, no_error_code);
exception_stub!(exc_stub_17, 17, has_error_code);
exception_stub!(exc_stub_18, 18, no_error_code);
exception_stub!(exc_stub_19, 19, no_error_code);
exception_stub!(exc_stub_20, 20, no_error_code);
exception_stub!(exc_stub_21, 21, has_error_code);
exception_stub!(exc_stub_22, 22, no_error_code);
exception_stub!(exc_stub_23, 23, no_error_code);
exception_stub!(exc_stub_24, 24, no_error_code);
exception_stub!(exc_stub_25, 25, no_error_code);
exception_stub!(exc_stub_26, 26, no_error_code);
exception_stub!(exc_stub_27, 27, no_error_code);
exception_stub!(exc_stub_28, 28, no_error_code);
exception_stub!(exc_stub_29, 29, has_error_code);
exception_stub!(exc_stub_30, 30, has_error_code);
exception_stub!(exc_stub_31, 31, no_error_code);

fn initialize_exception_idt(storage: *mut IdtEntry) {
    let handlers: [extern "sysv64" fn() -> !; EXCEPTION_IDT_ENTRY_COUNT] = [
        exc_stub_00,
        exc_stub_01,
        exc_stub_02,
        exc_stub_03,
        exc_stub_04,
        exc_stub_05,
        exc_stub_06,
        exc_stub_07,
        exc_stub_08,
        exc_stub_09,
        exc_stub_10,
        exc_stub_11,
        exc_stub_12,
        exc_stub_13,
        exc_stub_14,
        exc_stub_15,
        exc_stub_16,
        exc_stub_17,
        exc_stub_18,
        exc_stub_19,
        exc_stub_20,
        exc_stub_21,
        exc_stub_22,
        exc_stub_23,
        exc_stub_24,
        exc_stub_25,
        exc_stub_26,
        exc_stub_27,
        exc_stub_28,
        exc_stub_29,
        exc_stub_30,
        exc_stub_31,
    ];

    for (index, handler) in handlers.into_iter().enumerate() {
        let ist_index = if index == DOUBLE_FAULT_VECTOR {
            DOUBLE_FAULT_IST_INDEX
        } else {
            0
        };

        unsafe {
            storage.add(index).write(gate(handler, ist_index));
        }
    }
}

#[used]
#[unsafe(link_section = ".data.idt")]
static mut IDT: [IdtEntry; IDT_ENTRY_COUNT] = [IdtEntry::MISSING; IDT_ENTRY_COUNT];

#[repr(C)]
struct ExceptionFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    vector: u64,
    error_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

/// Common exception entry: save the full context, hand it to Rust, then either
/// resume the interrupted context or never come back.
///
/// The handler is allowed to *return*, and to have rewritten `rip` in the saved
/// frame before doing so. That is what makes a fault recoverable, which the
/// memory-protection proofs need (deliberately fault, then continue) and which
/// user-mode fault handling will need for real.
#[unsafe(naked)]
extern "sysv64" fn common_exception_entry() -> ! {
    naked_asm!(
        "cld",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // RDI = &ExceptionFrame. RBP keeps the unaligned RSP across the call,
        // which the SysV ABI requires to be 16-byte aligned; RBP itself is
        // already saved on the stack, so clobbering the register is free.
        "mov rdi, rsp",
        "mov rbp, rsp",
        "and rsp, -16",
        "call {handler}",
        "mov rsp, rbp",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        // Discard the vector and error-code words the stubs pushed.
        "add rsp, 16",
        "iretq",
        handler = sym handle_exception,
    )
}

/// A fault the kernel asked for on purpose, and what the CPU reported for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CaughtFault {
    pub vector: u8,
    pub error_code: u64,
    /// CR2 for #PF, otherwise the faulting instruction pointer.
    pub address: u64,
    pub rip: u64,
}

static EXPECT_ARMED: AtomicBool = AtomicBool::new(false);
static EXPECT_VECTOR: AtomicU8 = AtomicU8::new(0);
static EXPECT_ADDRESS_LOW: AtomicU64 = AtomicU64::new(0);
static EXPECT_ADDRESS_HIGH: AtomicU64 = AtomicU64::new(0);
static EXPECT_RECOVERY_RIP: AtomicU64 = AtomicU64::new(0);

static CAUGHT_ANY: AtomicBool = AtomicBool::new(false);
static CAUGHT_VECTOR: AtomicU8 = AtomicU8::new(0);
static CAUGHT_ERROR_CODE: AtomicU64 = AtomicU64::new(0);
static CAUGHT_ADDRESS: AtomicU64 = AtomicU64::new(0);
static CAUGHT_RIP: AtomicU64 = AtomicU64::new(0);

/// The two words a fault probe arms from assembly, once it knows the address
/// of its own recovery label.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExpectedFaultSlots {
    /// Where to resume. Must be written before `armed`.
    pub recovery_rip: *mut u64,
    /// Written last: with this set, a matching fault resumes instead of halting.
    pub armed: *mut bool,
}

/// Declare which fault is about to be provoked on purpose.
///
/// The caller finishes arming from assembly by storing its recovery label into
/// [`ExpectedFaultSlots::recovery_rip`] and then `1` into
/// [`ExpectedFaultSlots::armed`], in that order, immediately before the
/// faulting instruction. Splitting it this way is what lets the recovery point
/// be a label *after* the faulting instruction in the same `asm!` block.
///
/// The window is deliberately narrow: any *other* fault, including the same
/// vector at a different address, still takes the normal fatal path. An armed
/// expectation is consumed by the first matching fault.
pub(crate) fn prepare_expected_fault(vector: u8, low: u64, high: u64) -> ExpectedFaultSlots {
    EXPECT_ARMED.store(false, Ordering::Relaxed);
    CAUGHT_ANY.store(false, Ordering::Relaxed);
    EXPECT_VECTOR.store(vector, Ordering::Relaxed);
    EXPECT_ADDRESS_LOW.store(low, Ordering::Relaxed);
    EXPECT_ADDRESS_HIGH.store(high, Ordering::Relaxed);
    EXPECT_RECOVERY_RIP.store(0, Ordering::Release);

    ExpectedFaultSlots {
        recovery_rip: EXPECT_RECOVERY_RIP.as_ptr(),
        armed: EXPECT_ARMED.as_ptr(),
    }
}

/// Disarm without consuming, and report whether a matching fault was caught.
pub(crate) fn take_expected_fault() -> Option<CaughtFault> {
    EXPECT_ARMED.store(false, Ordering::Release);
    if !CAUGHT_ANY.swap(false, Ordering::AcqRel) {
        return None;
    }
    Some(CaughtFault {
        vector: CAUGHT_VECTOR.load(Ordering::Relaxed),
        error_code: CAUGHT_ERROR_CODE.load(Ordering::Relaxed),
        address: CAUGHT_ADDRESS.load(Ordering::Relaxed),
        rip: CAUGHT_RIP.load(Ordering::Relaxed),
    })
}

const PAGE_FAULT_VECTOR: u8 = 14;

fn read_cr2() -> u64 {
    let value: u64;
    // SAFETY: reading CR2 at CPL0 has no side effects.
    unsafe { asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags)) };
    value
}

/// Write the short name of a CPU exception vector to the debug console.
///
/// This reads the shared `aw_x86_interrupts::exception_name` table directly.
/// That was impossible while the kernel was a base-0-linked flat image loaded
/// at an arbitrary address: a `match` returning `&'static str` compiles to a
/// rodata pointer table whose entries are absolute addresses, which read back
/// as zero bytes and printed blank names. The image is now linked at, and
/// loaded at, the fixed base declared in its `AWKN` header, so absolute
/// references resolve correctly and the host-tested lookup is the single source
/// of truth again.
fn debug_exception_name(vector: u8) {
    debug_write(aw_x86_interrupts::exception_name(vector));
}

/// The bootstrap processor's own #DF IST bounds `[start, top)`.
pub(crate) fn bootstrap_ist1_bounds() -> (u64, u64) {
    // SAFETY: only the address of the field is taken, never its contents.
    let start = unsafe { core::ptr::addr_of!((*core::ptr::addr_of!(DOUBLE_FAULT_IST_STACK)).stack) }
        as *const u8 as u64;
    (start, start + DOUBLE_FAULT_IST_STACK_SIZE as u64)
}

/// The #DF IST bounds to check a fault frame against: this CPU's own, taken from
/// its per-CPU block when one is installed (an application processor, or the
/// bootstrap processor once it has one), and the bootstrap static otherwise - so
/// the early bootstrap double-fault smoke, which runs before any per-CPU block
/// exists, still checks the right stack.
fn double_fault_ist_bounds() -> (u64, u64) {
    if let Some(block) = crate::percpu::current()
        && let Some(bounds) = block.ist1_bounds()
    {
        return bounds;
    }
    bootstrap_ist1_bounds()
}

extern "sysv64" fn handle_exception(frame: *mut ExceptionFrame) {
    // SAFETY: `common_exception_entry` passes the address of the register block
    // it just pushed, which is a live, correctly laid out `ExceptionFrame`.
    let frame = unsafe { &mut *frame };
    let vector = frame.vector as u8;

    if let Some(recovery) = match_expected_fault(vector, frame) {
        // Resume the interrupted context at the recovery point instead of
        // halting. `iretq` restores the original RSP from the frame, so the
        // stack is exactly as the faulting instruction left it.
        frame.rip = recovery;
        return;
    }

    debug_write("AW_NATIVE_EXCEPTION vector=");
    crate::debug_write_u8(vector);
    debug_write(" name=");
    debug_exception_name(vector);
    debug_write(" error_code=");
    debug_write_hex_u64(frame.error_code);
    debug_write(" rip=");
    debug_write_hex_u64(frame.rip);
    debug_write(" cs=");
    debug_write_hex_u64(frame.cs);
    debug_write(" rflags=");
    debug_write_hex_u64(frame.rflags);
    debug_write("\n");

    // Dossier section 5.4: a fatal fault must journal RSP/CR2/CR3 too. CR2 is
    // only meaningful for a page fault, but printing it always keeps the fatal
    // record uniform.
    debug_write("AW_NATIVE_EXCEPTION_STATE rsp=");
    debug_write_hex_u64(frame.rsp);
    debug_write(" cr2=");
    debug_write_hex_u64(read_cr2());
    debug_write(" cr3=");
    debug_write_hex_u64(crate::virtual_memory::current_cr3());
    debug_write("\n");

    if vector == 6 {
        debug_write("AW_INVALID_OPCODE_HANDLER_OK\n");
    }

    if vector == 8 {
        let frame_address = frame as *const ExceptionFrame as u64;
        let (stack_start, stack_top) = double_fault_ist_bounds();

        debug_write("AW_DOUBLE_FAULT_FRAME address=");
        debug_write_hex_u64(frame_address);
        debug_write(" ist_start=");
        debug_write_hex_u64(stack_start);
        debug_write(" ist_top=");
        debug_write_hex_u64(stack_top);
        debug_write("\n");

        if frame_address >= stack_start && frame_address < stack_top {
            debug_write("AW_DOUBLE_FAULT_IST_OK\n");
        } else {
            debug_write("AW_DOUBLE_FAULT_IST_FAIL\n");
        }
    }

    halt_forever();
}

/// Reload every segment register from the kernel GDT.
///
/// `lgdt` only swaps the table; the CPU keeps using the descriptor caches
/// loaded by the firmware. Leaving them stale works by accident right up to the
/// first `iretq`, which re-validates the saved `CS`/`SS` **selectors** against
/// the live GDT: a firmware selector such as `0x38` is past the seven-entry
/// kernel table, so the return from the very first hardware interrupt raises
/// `#GP(0x38)` instead of resuming. A far return through the kernel code
/// selector, followed by explicit data-segment loads, makes the visible
/// selectors match the table the CPU now consults.
///
/// # Safety
///
/// Must run at CPL0 immediately after `lgdt`, with interrupts masked, on a
/// GDT whose code selector is a 64-bit Ring 0 code segment and whose data
/// selector is a writable Ring 0 data segment.
unsafe fn reload_segment_registers() {
    unsafe {
        asm!(
            // Far return: pushes the target CS and RIP, then reloads both.
            "push {code}",
            "lea {target}, [rip + 2f]",
            "push {target}",
            "retfq",
            "2:",
            // SS must be reloaded too: `iretq` validates the saved SS selector
            // exactly like CS.
            "mov ss, {data:e}",
            "mov ds, {data:e}",
            "mov es, {data:e}",
            "mov fs, {data:e}",
            "mov gs, {data:e}",
            code = in(reg) u64::from(CODE_SELECTOR_RAW),
            data = in(reg) u32::from(DATA_SELECTOR_RAW),
            target = out(reg) _,
            options(preserves_flags),
        );
    }
}

fn current_segments() -> (u16, u16) {
    let cs: u16;
    let ss: u16;
    // SAFETY: Reading segment selectors has no side effects.
    unsafe {
        asm!(
            "mov {cs:x}, cs",
            "mov {ss:x}, ss",
            cs = out(reg) cs,
            ss = out(reg) ss,
            options(nomem, nostack, preserves_flags),
        );
    }
    (cs, ss)
}

/// If this fault is the one the kernel armed for, record it and return where
/// execution must resume.
fn match_expected_fault(vector: u8, frame: &ExceptionFrame) -> Option<u64> {
    if !EXPECT_ARMED.load(Ordering::Acquire) || vector != EXPECT_VECTOR.load(Ordering::Relaxed) {
        return None;
    }
    let recovery = EXPECT_RECOVERY_RIP.load(Ordering::Acquire);
    if recovery == 0 {
        // Armed but never told where to resume: fall through to the fatal path
        // rather than `iretq` into address zero.
        return None;
    }

    let address = if vector == PAGE_FAULT_VECTOR {
        read_cr2()
    } else {
        frame.rip
    };
    if address < EXPECT_ADDRESS_LOW.load(Ordering::Relaxed)
        || address >= EXPECT_ADDRESS_HIGH.load(Ordering::Relaxed)
    {
        return None;
    }

    CAUGHT_VECTOR.store(vector, Ordering::Relaxed);
    CAUGHT_ERROR_CODE.store(frame.error_code, Ordering::Relaxed);
    CAUGHT_ADDRESS.store(address, Ordering::Relaxed);
    CAUGHT_RIP.store(frame.rip, Ordering::Relaxed);
    CAUGHT_ANY.store(true, Ordering::Relaxed);
    EXPECT_ARMED.store(false, Ordering::Release);

    Some(recovery)
}

fn prepare_tss_and_gdt() -> (DescriptorTablePointer, u64) {
    let (stack_start, stack_end) = double_fault_ist_bounds();
    debug_assert!(stack_start > double_fault_guard_page());
    let stack_top = stack_end & !0xf_u64;
    let tss_address = core::ptr::addr_of_mut!(TSS) as u64;
    let (tss_low, tss_high) = encode_tss_descriptor(tss_address);

    unsafe {
        core::ptr::addr_of_mut!(TSS).write(TaskStateSegment::with_ist1(stack_top));

        let gdt = core::ptr::addr_of_mut!(GDT).cast::<u64>();
        gdt.add(0).write(0);
        gdt.add(1).write(CODE_DESCRIPTOR);
        gdt.add(2).write(DATA_DESCRIPTOR);
        gdt.add(3).write(tss_low);
        gdt.add(4).write(tss_high);
        gdt.add(5).write(USER_DATA_DESCRIPTOR);
        gdt.add(6).write(USER_CODE_DESCRIPTOR);
    }

    let gdt_pointer = match DescriptorTablePointer::new(
        core::ptr::addr_of!(GDT) as u64,
        GDT_ENTRY_COUNT,
        core::mem::size_of::<u64>(),
    ) {
        Ok(pointer) => pointer,
        Err(_) => halt_forever(),
    };

    (gdt_pointer, stack_top)
}

/// Set the bootstrap processor's `TSS.rsp0`: the kernel stack the CPU switches
/// to on a privilege change into Ring 0 (an interrupt or exception taken while
/// running Ring 3 code). Ring 3 must not be entered before this is set, or such
/// a transition would push its frame through a null stack pointer.
///
/// # Safety
///
/// CPL0, single core. `rsp0` must be the 16-byte-aligned top of a writable
/// kernel stack that stays valid for as long as Ring 3 can run.
pub(crate) unsafe fn set_bootstrap_rsp0(rsp0: u64) {
    // SAFETY: single core; the TSS is not mid-task-switch, so writing rsp0 is a
    // plain memory store the CPU reads only on the next privilege change.
    unsafe {
        (*core::ptr::addr_of_mut!(TSS)).rsp0 = rsp0;
    }
}

/// Install the GDT, TSS/IST and exception-only IDT.
///
/// # Safety
///
/// Must run once with external interrupts masked.
pub(crate) unsafe fn install() {
    debug_write("AW_GDT_IDT_BEGIN\n");

    let (gdt_pointer, ist1_top) = prepare_tss_and_gdt();
    let idt_storage = core::ptr::addr_of_mut!(IDT).cast::<IdtEntry>();

    initialize_exception_idt(idt_storage);

    let idt_pointer = match DescriptorTablePointer::new(
        idt_storage as u64,
        IDT_ENTRY_COUNT,
        core::mem::size_of::<IdtEntry>(),
    ) {
        Ok(pointer) => pointer,
        Err(_) => halt_forever(),
    };

    unsafe {
        asm!(
            "lgdt [{gdt_pointer}]",
            gdt_pointer = in(reg) &gdt_pointer,
            options(readonly, nostack, preserves_flags),
        );
    }
    debug_write("AW_GDT_LOADED\n");

    // SAFETY: CPL0, interrupts masked, and the table just loaded above provides
    // the Ring 0 code/data descriptors this reload selects.
    unsafe { reload_segment_registers() };

    let (loaded_cs, loaded_ss) = current_segments();
    debug_write("AW_GDT_SEGMENTS_RELOADED cs=");
    debug_write_hex_u64(u64::from(loaded_cs));
    debug_write(" ss=");
    debug_write_hex_u64(u64::from(loaded_ss));
    debug_write("\n");
    if loaded_cs != CODE_SELECTOR_RAW || loaded_ss != DATA_SELECTOR_RAW {
        debug_write("AW_GDT_SEGMENTS_FAIL\n");
        halt_forever();
    }

    unsafe {
        asm!(
            "mov ax, 0x18",
            "ltr ax",
            out("ax") _,
            options(nostack, preserves_flags),
        );
    }

    let loaded_tr: u16;
    unsafe {
        asm!(
            "str ax",
            out("ax") loaded_tr,
            options(nostack, preserves_flags),
        );
    }

    debug_write("AW_TSS_LOADED tr=");
    debug_write_hex_u64(u64::from(loaded_tr));
    debug_write(" ist1_top=");
    debug_write_hex_u64(ist1_top);
    debug_write("\n");

    if loaded_tr != TSS_SELECTOR_RAW {
        debug_write("AW_TSS_LOAD_FAIL\n");
        halt_forever();
    }

    unsafe {
        asm!(
            "lidt [{idt_pointer}]",
            idt_pointer = in(reg) &idt_pointer,
            options(readonly, nostack, preserves_flags),
        );
    }

    debug_write("AW_IDT_LOADED\n");
    debug_write("AW_TSS_IST_READY vector=8 ist=1\n");
    debug_write("AW_IDT_VECTOR_COUNT ");
    crate::debug_write_u8(EXCEPTION_IDT_ENTRY_COUNT as u8);
    debug_write("/256\n");
    debug_write("AW_NATIVE_GDT_IDT_INSTALLED\n");
}

/// Wire an external or software interrupt vector (>= 32) into the live IDT.
///
/// The IDT was loaded with `lidt` from the address of the static `IDT` array,
/// so writing a fresh descriptor here updates the very table the CPU consults
/// on the next interrupt. The gate is built with the same audited
/// [`IdtEntry`] encoder used for the exception vectors, instead of an
/// open-coded descriptor, so there is a single source of truth for the gate
/// byte layout.
///
/// # Safety
///
/// Must run at CPL0 after [`install()`], with interrupts masked while the
/// descriptor is replaced. `handler_addr` must point at a valid naked
/// interrupt entry that preserves all registers and returns with `iretq`.
/// `ist_index` selects `TSS.IST[ist_index]` (0 keeps the current stack).
pub(crate) unsafe fn install_interrupt_gate(
    vector: u8,
    handler_addr: u64,
    ist_index: u8,
) -> Result<(), IdtEntryError> {
    let entry = IdtEntry::new(
        handler_addr,
        CODE_SELECTOR,
        GateType::Interrupt,
        PrivilegeLevel::Ring0,
        ist_index,
    )?;

    // SAFETY: `vector` is a u8, so it always indexes within the 256-entry IDT.
    // The caller guarantees CPL0 execution after the table has been loaded.
    unsafe {
        core::ptr::addr_of_mut!(IDT)
            .cast::<IdtEntry>()
            .add(vector as usize)
            .write_volatile(entry);
    }

    Ok(())
}

/// Force a deterministic double fault:
///
/// 1. Replace the #GP gate's code selector with selector 0x18, which refers to
///    the TSS system descriptor and is therefore invalid as a code segment.
/// 2. Execute `mov ds, 0x18`, producing a real #GP.
/// 3. Delivering that #GP faults again while loading its invalid gate selector,
///    so the CPU must enter #DF (vector 8), whose gate uses IST1.
///
/// # Safety
///
/// Only call in the dedicated smoke-test build after `install()`.
#[cfg(any(feature = "double-fault-smoke-test", feature = "ap-double-fault-smoke-test"))]
pub(crate) unsafe fn trigger_double_fault_smoke() -> ! {
    let tss_selector = match SegmentSelector::new(TSS_SELECTOR_INDEX, PrivilegeLevel::Ring0) {
        Some(value) => value,
        None => halt_forever(),
    };

    let poisoned_gp_gate =
        gate_for_selector(exc_stub_13 as *const () as usize as u64, tss_selector, 0);

    let idt_storage = core::ptr::addr_of_mut!(IDT).cast::<IdtEntry>();
    unsafe {
        idt_storage
            .add(GENERAL_PROTECTION_VECTOR)
            .write(poisoned_gp_gate);
    }

    debug_write("AW_DOUBLE_FAULT_SMOKE_ARMED gp_gate_selector=0x18 df_ist=1\n");
    debug_write("AW_DOUBLE_FAULT_SMOKE_TRIGGER initial=general-protection\n");

    unsafe {
        asm!("mov ax, 0x18", "mov ds, ax", "ud2", options(noreturn),);
    }
}
