//! Deliberate faults that prove the kernel's memory protections are real.
//!
//! Section 7 of the dossier is explicit: W^X, NX and guard pages must not be
//! claimed on the strength of page-table flags or a successful build. They are
//! claimed only once the expected faults have actually been observed. This
//! module performs the three negative tests on the live CPU, immediately after
//! the kernel's own page tables go active:
//!
//! 1. executing a data page must fault (NX),
//! 2. writing to `.text` must fault (W^X, via CR0.WP),
//! 3. touching the page below the #DF stack must fault (guard page).
//!
//! Each probe arms a narrow expectation in the exception handler, performs the
//! offending access, and resumes at a recovery label. A probe that does *not*
//! fault returns `None`, which the caller reports as a failure - the dangerous
//! outcome here is silence, not a crash.

use core::arch::asm;

use crate::interrupts::{prepare_expected_fault, take_expected_fault, CaughtFault};

const PAGE_FAULT_VECTOR: u8 = 14;
const PAGE_SIZE: u64 = 4096;

/// `#PF` error-code bits (Intel SDM 4.7).
pub mod page_fault {
    /// 0 = the fault was caused by a non-present page.
    pub const PRESENT: u64 = 1 << 0;
    /// 1 = the access was a write.
    pub const WRITE: u64 = 1 << 1;
    /// 1 = the access came from user mode.
    pub const USER: u64 = 1 << 2;
    /// 1 = the fault was an instruction fetch (requires NX enabled).
    pub const INSTRUCTION_FETCH: u64 = 1 << 4;
}

/// One protection the kernel claims, and the fault that backs the claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtectionProof {
    pub name: &'static str,
    pub outcome: ProofOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProofOutcome {
    /// The expected fault happened, with the expected error-code bits.
    Faulted(CaughtFault),
    /// The access went through. The protection is not in force.
    NoFault,
    /// A fault happened but its error code does not describe the violation the
    /// probe was testing for, so it does not prove the protection.
    WrongErrorCode(CaughtFault),
}

impl ProofOutcome {
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Faulted(_))
    }
}

fn classify(caught: Option<CaughtFault>, required: u64, forbidden: u64) -> ProofOutcome {
    match caught {
        None => ProofOutcome::NoFault,
        Some(fault) => {
            if fault.error_code & required == required && fault.error_code & forbidden == 0 {
                ProofOutcome::Faulted(fault)
            } else {
                ProofOutcome::WrongErrorCode(fault)
            }
        }
    }
}

/// Try to execute the page at `address`, which must be mapped NX.
///
/// # Safety
///
/// `address` must be inside a mapped, non-executable page the kernel owns. If
/// NX is not in force the CPU executes whatever bytes are there, which is why
/// this must only ever be pointed at kernel-owned data.
pub unsafe fn probe_execute_data(address: u64) -> ProofOutcome {
    let slots = prepare_expected_fault(PAGE_FAULT_VECTOR, address, address + PAGE_SIZE);

    // SAFETY: the recovery label sits immediately after the faulting `jmp` in
    // this same block, and `jmp` pushes nothing, so `iretq` resumes there with
    // the stack exactly as it was. Arming is completed here, recovery address
    // first and the armed flag last, so a fault can never be accepted without a
    // valid resume point.
    unsafe {
        asm!(
            "lea {scratch}, [rip + 2f]",
            "mov qword ptr [{recovery}], {scratch}",
            "mov byte ptr [{armed}], 1",
            "jmp {target}",
            "2:",
            scratch = out(reg) _,
            recovery = in(reg) slots.recovery_rip,
            armed = in(reg) slots.armed,
            target = in(reg) address,
            options(nostack),
        );
    }

    classify(
        take_expected_fault(),
        page_fault::INSTRUCTION_FETCH | page_fault::PRESENT,
        page_fault::USER | page_fault::WRITE,
    )
}

/// Try to write to `address`, which must be mapped read-only.
///
/// # Safety
///
/// `address` must be inside a read-only page the kernel owns. If CR0.WP is
/// clear the write succeeds and corrupts that byte, which is precisely the
/// state this probe exists to detect.
pub unsafe fn probe_write_readonly(address: u64) -> ProofOutcome {
    let slots = prepare_expected_fault(PAGE_FAULT_VECTOR, address, address + PAGE_SIZE);

    // SAFETY: see `probe_execute_data`; the faulting store touches no stack.
    unsafe {
        asm!(
            "lea {scratch}, [rip + 2f]",
            "mov qword ptr [{recovery}], {scratch}",
            "mov byte ptr [{armed}], 1",
            "mov byte ptr [{target}], 0",
            "2:",
            scratch = out(reg) _,
            recovery = in(reg) slots.recovery_rip,
            armed = in(reg) slots.armed,
            target = in(reg) address,
            options(nostack),
        );
    }

    classify(
        take_expected_fault(),
        page_fault::WRITE | page_fault::PRESENT,
        page_fault::USER | page_fault::INSTRUCTION_FETCH,
    )
}

/// Try to read `address`, which must be an unmapped guard page.
///
/// # Safety
///
/// `address` must be a page the kernel's map deliberately leaves unmapped.
pub unsafe fn probe_touch_guard_page(address: u64) -> ProofOutcome {
    let slots = prepare_expected_fault(PAGE_FAULT_VECTOR, address, address + PAGE_SIZE);

    // SAFETY: see `probe_execute_data`; the faulting load touches no stack.
    unsafe {
        asm!(
            "lea {scratch}, [rip + 2f]",
            "mov qword ptr [{recovery}], {scratch}",
            "mov byte ptr [{armed}], 1",
            "mov {scratch}, qword ptr [{target}]",
            "2:",
            scratch = out(reg) _,
            recovery = in(reg) slots.recovery_rip,
            armed = in(reg) slots.armed,
            target = in(reg) address,
            options(nostack),
        );
    }

    // A not-present fault must NOT have the PRESENT bit set.
    classify(
        take_expected_fault(),
        0,
        page_fault::PRESENT | page_fault::USER,
    )
}
