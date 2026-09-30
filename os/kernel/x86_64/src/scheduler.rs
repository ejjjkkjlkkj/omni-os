//! Cooperative round-robin scheduler with real context switching (dossier
//! section 8, roadmap P0 step 4).
//!
//! Ring 3 and syscalls proved the privilege boundary; this proves the other
//! half of step 4 - more than one thread of execution, switched by the kernel.
//! Several kernel threads each run on their own stack and hand control on with
//! `yield_now`; the switch saves the current thread's callee-saved registers and
//! stack pointer and restores the next thread's, so each resumes exactly where
//! it left off. The preemptive half - threads switched by the timer interrupt
//! without yielding - is built on the same idea further down (`prove_preemptive`).
//!
//! It runs on the bootstrap processor with interrupts masked, so the shared
//! state needs no lock; the counters are atomics only so the assembly switch and
//! the Rust readers agree on memory.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::{debug_write, debug_write_u64};

const MAX_THREADS: usize = 3;
const STACK_SIZE: usize = 16 * 1024;
/// Total yields after which the running thread returns control to the kernel.
const TARGET_YIELDS: u32 = 30;

#[repr(C, align(16))]
struct Stack([u8; STACK_SIZE]);

static mut STACKS: [Stack; MAX_THREADS] = [const { Stack([0; STACK_SIZE]) }; MAX_THREADS];

/// Saved stack pointer per thread (updated by the assembly switch), the kernel's
/// own saved pointer, and the round-robin bookkeeping.
static THREAD_RSP: [AtomicU64; MAX_THREADS] = [const { AtomicU64::new(0) }; MAX_THREADS];
static MAIN_RSP: AtomicU64 = AtomicU64::new(0);
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static NUM_THREADS: AtomicUsize = AtomicUsize::new(0);
static COUNTS: [AtomicU32; MAX_THREADS] = [const { AtomicU32::new(0) }; MAX_THREADS];
static TOTAL_YIELDS: AtomicU32 = AtomicU32::new(0);
static SWITCHES: AtomicU32 = AtomicU32::new(0);

core::arch::global_asm!(
    ".global aw_context_switch",
    ".type aw_context_switch,@function",
    // fn aw_context_switch(save_rsp = rdi, load_rsp = rsi)
    "aw_context_switch:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov [rdi], rsp", // save the outgoing thread's stack pointer
    "    mov rsp, rsi",   // load the incoming thread's stack pointer
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    ret",
    ".size aw_context_switch, .-aw_context_switch",
);

unsafe extern "C" {
    fn aw_context_switch(save_rsp: *mut u64, load_rsp: u64);
}

/// Lay out a thread's initial stack so the first switch into it "returns" to
/// `entry`, with zeroed callee-saved registers.
fn init_thread(index: usize, entry: extern "C" fn() -> !) {
    // SAFETY: `index < MAX_THREADS`; this addresses that thread's own stack.
    let top = unsafe {
        let base = core::ptr::addr_of_mut!(STACKS).cast::<Stack>().add(index) as u64;
        (base + STACK_SIZE as u64) & !0xf_u64
    };
    // Seven qwords: r15,r14,r13,r12,rbx,rbp (popped by the switch) then the
    // return address the final `ret` jumps to.
    let sp = top - 7 * 8;
    let slots = sp as *mut u64;
    // SAFETY: [sp, top) is inside this thread's stack; write the seven slots.
    unsafe {
        for offset in 0..6 {
            slots.add(offset).write(0);
        }
        slots.add(6).write(entry as usize as u64);
    }
    THREAD_RSP[index].store(sp, Ordering::Relaxed);
}

/// Hand control to the next thread in round-robin order.
fn yield_now() {
    let current = CURRENT.load(Ordering::Relaxed);
    let count = NUM_THREADS.load(Ordering::Relaxed);
    let next = (current + 1) % count;
    CURRENT.store(next, Ordering::Relaxed);
    SWITCHES.fetch_add(1, Ordering::Relaxed);
    // SAFETY: both stack pointers belong to live threads set up by init_thread;
    // the switch saves this thread's context and restores the next thread's.
    unsafe {
        aw_context_switch(
            THREAD_RSP[current].as_ptr(),
            THREAD_RSP[next].load(Ordering::Relaxed),
        )
    };
}

/// Return control to the kernel that called [`run`].
fn exit_to_main() -> ! {
    let current = CURRENT.load(Ordering::Relaxed);
    // SAFETY: save this (now finished) thread's pointer and load the kernel's.
    unsafe {
        aw_context_switch(
            THREAD_RSP[current].as_ptr(),
            MAIN_RSP.load(Ordering::Relaxed),
        )
    };
    // The kernel never switches back to this thread.
    loop {
        core::hint::spin_loop();
    }
}

/// Every thread's body: advance its own counter, yield, and once the whole run
/// has done enough yields, hand control back to the kernel.
fn thread_body(id: usize) -> ! {
    loop {
        COUNTS[id].fetch_add(1, Ordering::Relaxed);
        let total = TOTAL_YIELDS.fetch_add(1, Ordering::Relaxed) + 1;
        if total >= TARGET_YIELDS {
            exit_to_main();
        }
        yield_now();
    }
}

extern "C" fn thread0() -> ! {
    thread_body(0)
}
extern "C" fn thread1() -> ! {
    thread_body(1)
}
extern "C" fn thread2() -> ! {
    thread_body(2)
}

/// Start the threads and run until they hand control back.
fn run() {
    init_thread(0, thread0);
    init_thread(1, thread1);
    init_thread(2, thread2);
    NUM_THREADS.store(3, Ordering::Relaxed);
    CURRENT.store(0, Ordering::Relaxed);
    // SAFETY: save the kernel's context into MAIN_RSP and switch to thread 0.
    unsafe { aw_context_switch(MAIN_RSP.as_ptr(), THREAD_RSP[0].load(Ordering::Relaxed)) };
}

/// Prove cooperative multitasking: three threads interleave deterministically.
pub fn prove() {
    debug_write("AW_SCHED_BEGIN\n");
    run();

    // Control is back in the kernel. With three threads and 30 total yields,
    // round-robin gives each thread exactly ten turns.
    let mut all_ran = true;
    for (id, count_slot) in COUNTS.iter().enumerate() {
        let count = count_slot.load(Ordering::Relaxed);
        debug_write("AW_SCHED_THREAD id=");
        debug_write_u64(id as u64);
        debug_write(" count=");
        debug_write_u64(u64::from(count));
        debug_write("\n");
        if count != TARGET_YIELDS / 3 {
            all_ran = false;
        }
    }

    let total = TOTAL_YIELDS.load(Ordering::Relaxed);
    let switches = SWITCHES.load(Ordering::Relaxed);
    if all_ran && total == TARGET_YIELDS {
        crate::firmware_runtime::pass(aw_generation::RuntimeHealthCheck::Kernel);
        debug_write("AW_SCHED_PROOF_OK threads=3 switches=");
        debug_write_u64(u64::from(switches));
        debug_write("\n");
    } else {
        debug_write("AW_SCHED_FAIL total=");
        debug_write_u64(u64::from(total));
        debug_write("\n");
    }
}

// ---- Preemptive scheduling (dossier section 8, roadmap P0) -----------------
//
// The cooperative switch above proves threads can be interleaved when they yield.
// Preemption proves the harder half: threads that never yield are switched anyway,
// driven only by the timer interrupt. The same timer ISR that counts ticks calls
// `aw_preempt_pick` after signalling EOI; when preemption is active it saves the
// interrupted thread's full register frame (the ISR already pushed it) and returns
// another thread's saved frame, so the interrupt returns into a different thread.
//
// A runtime flag gates all of it: with the flag clear, `aw_preempt_pick` returns
// the frame it was handed, so every other proof that takes a timer interrupt runs
// exactly as before.

/// Kernel selectors, matching the GDT the bootstrap processor reloaded
/// (AW_GDT_SEGMENTS_RELOADED cs=0x08 ss=0x10).
const KERNEL_CODE_SELECTOR: u64 = 0x08;
const KERNEL_DATA_SELECTOR: u64 = 0x10;

const PREEMPT_THREADS: usize = 3;
/// Slot count: slot 0 is the kernel that starts a run, slots 1..=PREEMPT_THREADS
/// are the runnable contexts (kernel threads, or one user thread).
const PREEMPT_SLOTS: usize = PREEMPT_THREADS + 1;

static mut PREEMPT_STACKS: [Stack; PREEMPT_THREADS] =
    [const { Stack([0; STACK_SIZE]) }; PREEMPT_THREADS];

/// Whether the timer ISR should switch contexts. False everywhere except inside a
/// preemption proof, so no other interrupt path is affected.
static PREEMPT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Saved full-frame RSP per slot. Slot 0 is the kernel that started the run
/// (saved when it is first preempted); slots 1.. are the runnable contexts.
static PREEMPT_RSP: [AtomicU64; PREEMPT_SLOTS] = [const { AtomicU64::new(0) }; PREEMPT_SLOTS];
/// Per-slot kernel stack top for the TSS RSP0, so each CPL3 slot takes its
/// interrupts on its own kernel stack instead of colliding on one. Zero for slots
/// that run at CPL0 (the kernel and kernel threads), which never switch stacks on
/// an interrupt and so need no RSP0 update.
static PREEMPT_KSTACK: [AtomicU64; PREEMPT_SLOTS] = [const { AtomicU64::new(0) }; PREEMPT_SLOTS];
static PREEMPT_CURRENT: AtomicUsize = AtomicUsize::new(0);
static PREEMPT_SWITCHES: AtomicU32 = AtomicU32::new(0);
/// Timer-driven switches to perform before handing control back to slot 0.
static PREEMPT_LIMIT: AtomicU32 = AtomicU32::new(0);
/// The slots to rotate through, and how many are in use. `aw_preempt_pick` picks
/// the next runnable slot from here, so kernel-thread and user-thread proofs can
/// share one switcher with different rotations.
static PREEMPT_ROTATION: [AtomicUsize; PREEMPT_THREADS] =
    [const { AtomicUsize::new(0) }; PREEMPT_THREADS];
static PREEMPT_ROTATION_LEN: AtomicUsize = AtomicUsize::new(0);
/// Set whenever the context just preempted was running at CPL3 - the evidence
/// that a real user thread was interrupted by the timer.
static PREEMPT_SAW_USER: AtomicBool = AtomicBool::new(false);
static PREEMPT_COUNTS: [AtomicU64; PREEMPT_THREADS] =
    [const { AtomicU64::new(0) }; PREEMPT_THREADS];

/// Called by the timer ISR after EOI, with the interrupted context's full-frame
/// RSP. Returns the RSP to resume on - the same one when preemption is inactive,
/// another slot's saved frame when it is active.
#[unsafe(no_mangle)]
extern "C" fn aw_preempt_pick(current_rsp: u64) -> u64 {
    if !PREEMPT_ACTIVE.load(Ordering::Acquire) {
        return current_rsp;
    }
    let current = PREEMPT_CURRENT.load(Ordering::Relaxed);
    PREEMPT_RSP[current].store(current_rsp, Ordering::Relaxed);

    // The saved CS sits 128 bytes into the frame (15 GP registers, then rip).
    // CPL 3 there means a user context was just interrupted.
    // SAFETY: current_rsp is the live frame the ISR just built on a kernel stack.
    let cs = unsafe { ((current_rsp + 128) as *const u64).read_volatile() };
    if cs & 3 == 3 {
        PREEMPT_SAW_USER.store(true, Ordering::Relaxed);
    }

    let switches = PREEMPT_SWITCHES.fetch_add(1, Ordering::Relaxed) + 1;
    let next = if switches >= PREEMPT_LIMIT.load(Ordering::Relaxed) {
        // Enough preemptions: stop switching and return to slot 0 (the kernel).
        PREEMPT_ACTIVE.store(false, Ordering::Release);
        0
    } else {
        let len = PREEMPT_ROTATION_LEN.load(Ordering::Relaxed).max(1);
        PREEMPT_ROTATION[(switches as usize - 1) % len].load(Ordering::Relaxed)
    };
    PREEMPT_CURRENT.store(next, Ordering::Relaxed);

    // Point the TSS RSP0 at the incoming slot's own kernel stack, so when that
    // slot is a CPL3 thread its next interrupt lands on its own stack rather than
    // on top of another thread's saved frame. Slots that run at CPL0 leave this
    // zero and keep the RSP0 they had.
    let kstack = PREEMPT_KSTACK[next].load(Ordering::Relaxed);
    if kstack != 0 {
        // SAFETY: CPL0 in interrupt context; sets this CPU's TSS RSP0.
        unsafe { crate::interrupts::set_bootstrap_rsp0(kstack) };
    }

    PREEMPT_RSP[next].load(Ordering::Relaxed)
}

/// Configure the rotation and switch budget for the next run.
fn set_preempt_plan(rotation: &[usize], limit: u32) {
    for (slot, value) in PREEMPT_ROTATION.iter().zip(rotation.iter()) {
        slot.store(*value, Ordering::Relaxed);
    }
    PREEMPT_ROTATION_LEN.store(rotation.len(), Ordering::Relaxed);
    PREEMPT_LIMIT.store(limit, Ordering::Relaxed);
    PREEMPT_CURRENT.store(0, Ordering::Relaxed);
    PREEMPT_SWITCHES.store(0, Ordering::Relaxed);
    PREEMPT_SAW_USER.store(false, Ordering::Relaxed);
    // Per-slot kernel stacks are left as-is here: they default to zero (no RSP0
    // switch, correct for the CPL0 kernel-thread and single-user runs) and the
    // caller sets them just before a CPL3 multi-thread run. That run is the last
    // preemption run in a boot, so nothing stale can leak into an earlier one.
}

/// Install one slot's own kernel stack top for the TSS RSP0, required for a CPL3
/// thread so it takes interrupts on its own stack. Call after [`set_preempt_plan`]
/// (via `run_preemption_over`, which calls it), before the run begins.
pub fn set_slot_kstack(slot: usize, kstack_top: u64) {
    if let Some(cell) = PREEMPT_KSTACK.get(slot) {
        cell.store(kstack_top, Ordering::Relaxed);
    }
}

/// Activate preemption, let the timer drive the switches, and return once the
/// budget is spent and control is back on slot 0.
///
/// # Safety
/// CPL0 on the bootstrap processor, before any application processor is online
/// (the switch state is single-CPU), with the APIC timer gate installed. The
/// timer is left masked and interrupts disabled on return.
unsafe fn run_preemption() {
    PREEMPT_ACTIVE.store(true, Ordering::Release);
    // SAFETY: unmask the timer and enable interrupts so the ISR preempts this
    // loop; once PREEMPT_LIMIT switches have happened it returns here with the
    // flag cleared and the loop falls through.
    unsafe {
        crate::apic_timer::set_timer_masked(false);
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
    while PREEMPT_ACTIVE.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    // SAFETY: restore the masked-timer, interrupts-disabled state callers expect.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        crate::apic_timer::set_timer_masked(true);
    }
}

/// Run one preinstalled user context (slot 1) under timer preemption for `limit`
/// switches, then return to the kernel. Returns the number of switches performed
/// and whether a CPL3 context was ever the one preempted.
///
/// The caller installs slot 1's initial CPL3 frame in [`set_user_slot_frame`],
/// maps the user pages, sets TSS RSP0 and the GS bases, and reads back the user
/// thread's own evidence afterwards.
///
/// # Safety
/// Same as [`run_preemption`]; additionally slot 1 must hold a valid CPL3
/// interrupt frame and the swapgs bases must be armed.
pub unsafe fn run_user_preemption(limit: u32) -> (u32, bool) {
    // SAFETY: forwarded to the caller's contract.
    unsafe { run_preemption_over(&[1], limit) }
}

/// The preemption slot currently running (0 = kernel). A syscall from CPL3 runs
/// with interrupts masked, so this is the slot of the thread that trapped.
pub fn current_slot() -> usize {
    PREEMPT_CURRENT.load(Ordering::Relaxed)
}

/// Install slot 1's initial saved-frame RSP (a CPL3 interrupt frame the caller
/// built) for [`run_user_preemption`].
pub fn set_user_slot_frame(frame_rsp: u64) {
    set_slot_frame(1, frame_rsp);
}

/// Install one slot's initial saved-frame RSP (an interrupt frame the caller
/// built). Slots 1.. are the runnable contexts; slot 0 is the kernel.
pub fn set_slot_frame(slot: usize, frame_rsp: u64) {
    if let Some(cell) = PREEMPT_RSP.get(slot) {
        cell.store(frame_rsp, Ordering::Relaxed);
    }
}

/// Run the preinstalled slots in `rotation` under timer preemption for `limit`
/// switches, then return to the kernel. Returns the number of switches performed
/// and whether a CPL3 context was ever the one preempted.
///
/// # Safety
/// Same as [`run_preemption`]; every slot named in `rotation` must hold a valid
/// initial interrupt frame, and (for user slots) the swapgs bases must be armed.
pub unsafe fn run_preemption_over(rotation: &[usize], limit: u32) -> (u32, bool) {
    set_preempt_plan(rotation, limit);
    // SAFETY: forwarded to the caller's contract.
    unsafe { run_preemption() };
    (
        PREEMPT_SWITCHES.load(Ordering::Relaxed),
        PREEMPT_SAW_USER.load(Ordering::Relaxed),
    )
}

/// Prove cooperative multitasking runs on an application processor, not just the
/// bootstrap processor: two kernel threads context switch on this CPU and take
/// turns until the whole run has yielded enough (dossier section 8 - "anything
/// running on an AP").
///
/// It reuses the same context switch and thread state as the bootstrap proof; by
/// the time an AP runs this the bootstrap processor has long finished with them,
/// and only one AP runs it, so the shared statics need no lock.
///
/// # Safety
/// CPL0 on the application processor that calls it, once, with interrupts masked.
#[cfg(feature = "ap-scheduler-smoke-test")]
pub unsafe fn prove_ap_scheduler(cpu: usize) {
    debug_write("AW_AP_SCHED_BEGIN cpu=");
    debug_write_u64(cpu as u64);
    debug_write("\n");

    for count in &COUNTS {
        count.store(0, Ordering::Relaxed);
    }
    TOTAL_YIELDS.store(0, Ordering::Relaxed);
    SWITCHES.store(0, Ordering::Relaxed);
    init_thread(0, thread0);
    init_thread(1, thread1);
    NUM_THREADS.store(2, Ordering::Relaxed);
    CURRENT.store(0, Ordering::Relaxed);

    // SAFETY: save this AP's context into MAIN_RSP and switch to thread 0; the
    // threads hand control back here once TARGET_YIELDS is reached.
    unsafe { aw_context_switch(MAIN_RSP.as_ptr(), THREAD_RSP[0].load(Ordering::Relaxed)) };

    let switches = SWITCHES.load(Ordering::Relaxed);
    let total = TOTAL_YIELDS.load(Ordering::Relaxed);
    let both_ran = COUNTS[0].load(Ordering::Relaxed) > 0 && COUNTS[1].load(Ordering::Relaxed) > 0;
    if both_ran && total == TARGET_YIELDS {
        debug_write("AW_AP_SCHED_PROOF_OK cpu=");
        debug_write_u64(cpu as u64);
        debug_write(" threads=2 switches=");
        debug_write_u64(u64::from(switches));
        debug_write("\n");
    } else {
        debug_write("AW_AP_SCHED_FAIL cpu=");
        debug_write_u64(cpu as u64);
        debug_write("\n");
    }
}

/// Lay out a thread's initial stack as if it had just been interrupted, so the
/// timer ISR's own `pop`/`iretq` epilogue starts it at `entry` with interrupts
/// enabled. The layout mirrors the ISR prologue exactly: fifteen general-purpose
/// registers, then the CPU's interrupt frame (rip, cs, rflags, rsp, ss).
fn init_preempt_thread(index: usize, entry: extern "C" fn() -> !) {
    // SAFETY: index < PREEMPT_THREADS; this addresses that thread's own stack.
    let top = unsafe {
        let base = core::ptr::addr_of_mut!(PREEMPT_STACKS)
            .cast::<Stack>()
            .add(index) as u64;
        (base + STACK_SIZE as u64) & !0xf_u64
    };
    // 15 saved GP registers + 5 interrupt-frame slots = 20 qwords.
    let frame = top - 20 * 8;
    let slots = frame as *mut u64;
    // SAFETY: [frame, top) is inside this thread's stack.
    unsafe {
        for offset in 0..15 {
            slots.add(offset).write(0); // r15..rax, all zero
        }
        slots.add(15).write(entry as usize as u64); // rip
        slots.add(16).write(KERNEL_CODE_SELECTOR); // cs
        slots.add(17).write(0x202); // rflags: IF set, reserved bit 1
        slots.add(18).write(frame); // rsp the thread runs on after iretq
        slots.add(19).write(KERNEL_DATA_SELECTOR); // ss
    }
    PREEMPT_RSP[index + 1].store(frame, Ordering::Relaxed);
}

fn preempt_thread(id: usize) -> ! {
    loop {
        PREEMPT_COUNTS[id].fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

extern "C" fn preempt_thread0() -> ! {
    preempt_thread(0)
}
extern "C" fn preempt_thread1() -> ! {
    preempt_thread(1)
}
extern "C" fn preempt_thread2() -> ! {
    preempt_thread(2)
}

/// Prove preemptive multitasking: three threads that never yield are still
/// interleaved, driven only by timer interrupts.
///
/// # Safety
/// CPL0 on the bootstrap processor, after the APIC timer gate is installed and
/// x2APIC is enabled. The timer is left masked with interrupts disabled on
/// return, whatever the outcome.
pub unsafe fn prove_preemptive() {
    debug_write("AW_PREEMPT_BEGIN\n");

    init_preempt_thread(0, preempt_thread0);
    init_preempt_thread(1, preempt_thread1);
    init_preempt_thread(2, preempt_thread2);
    for count in &PREEMPT_COUNTS {
        count.store(0, Ordering::Relaxed);
    }

    // Rotate through the three thread slots; twelve switches gives each several
    // turns before control returns to slot 0 (this kernel path).
    set_preempt_plan(&[1, 2, 3], 12);
    // SAFETY: CPL0 on the bootstrap processor, timer gate installed, no AP online.
    unsafe { run_preemption() };

    let switches = PREEMPT_SWITCHES.load(Ordering::Relaxed);
    let mut all_ran = true;
    for (id, count_slot) in PREEMPT_COUNTS.iter().enumerate() {
        let count = count_slot.load(Ordering::Relaxed);
        debug_write("AW_PREEMPT_THREAD id=");
        debug_write_u64(id as u64);
        debug_write(" count=");
        debug_write_u64(count);
        debug_write("\n");
        if count == 0 {
            all_ran = false;
        }
    }

    // Each thread advanced without ever yielding, and control came back after
    // exactly the requested number of timer-driven switches: the switching was the
    // timer's doing, not the threads'.
    if all_ran && switches == 12 {
        debug_write("AW_PREEMPT_PROOF_OK threads=3 switches=");
        debug_write_u64(u64::from(switches));
        debug_write("\n");
    } else {
        debug_write("AW_PREEMPT_FAIL switches=");
        debug_write_u64(u64::from(switches));
        debug_write("\n");
    }
}
