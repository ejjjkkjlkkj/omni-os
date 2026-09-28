//! The delivery proof every interrupt source in this kernel has to survive.
//!
//! A counter that goes up is not evidence that an interrupt was delivered: a
//! polling loop, a mis-set flag or a self-inflicted call would produce exactly
//! the same trace. What distinguishes real delivery is that *masking the source
//! stops it*. This module runs that sequence once, so the local APIC timer, an
//! I/O APIC routed device IRQ and an MSI are all held to the same bar and the
//! negative test cannot quietly differ between them.
//!
//! The proof leaves interrupts disabled and the source masked whatever the
//! outcome, so a failure never hands a half-armed interrupt source back to the
//! rest of bring-up.

use core::arch::asm;

/// Bounded busy-wait budget for one phase of a delivery proof. Large enough for
/// several periods under QEMU/TCG, small enough that a dead source fails the
/// proof in seconds rather than hanging the boot.
pub const PROOF_SPIN_BUDGET: u32 = 200_000_000;

/// Outcome of [`run`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryProof {
    /// The handler ran at least `required_ticks` times, stopped while masked
    /// and resumed once unmasked.
    Passed {
        ticks_after_run: u64,
        ticks_while_masked: u64,
        ticks_after_unmask: u64,
    },
    /// The source was unmasked but the handler never reached `required_ticks`.
    NotDelivered { ticks: u64 },
    /// The counter kept advancing while the source was masked, so the
    /// increments are not attributable to real interrupt delivery.
    MaskIneffective { before: u64, after: u64 },
    /// Delivery did not resume after clearing the mask.
    DidNotResume { ticks: u64 },
}

/// How often a source that does not run free is nudged while the proof waits.
const POKE_SPIN_INTERVAL: u32 = 1 << 20;

/// One interrupt source, as the proof needs to see it.
///
/// [`Self::poke`] exists because not every source runs free. A periodic timer
/// keeps firing on its own; a device fires when something asks it to. Both must
/// pass the same test, and for a poked source the negative test is stronger:
/// the proof keeps asking for interrupts throughout the masked window, so a
/// frozen counter there means the mask really suppressed deliveries that were
/// actually requested.
pub trait InterruptSource {
    /// Deliveries counted by this source's handler so far.
    fn ticks(&self) -> u64;

    /// Mask or unmask the source at its own controller - the local APIC's
    /// `LVT`, an I/O APIC redirection entry, or a device's MSI enable bit.
    fn set_masked(&self, masked: bool);

    /// Ask a non-free-running source for one more interrupt.
    fn poke(&self) {}
}

/// # Safety
/// The caller must ensure the rest of the kernel is ready for maskable IRQs.
pub unsafe fn enable_interrupts() {
    unsafe { asm!("sti", options(nomem, nostack, preserves_flags)) };
}

/// # Safety
/// CPL0 only.
pub unsafe fn disable_interrupts() {
    unsafe { asm!("cli", options(nomem, nostack, preserves_flags)) };
}

fn spin_until(
    source: &impl InterruptSource,
    budget: u32,
    mut done: impl FnMut() -> bool,
) -> bool {
    source.poke();
    let mut spins = 0;
    while spins < budget {
        if done() {
            return true;
        }
        spins += 1;
        if spins.is_multiple_of(POKE_SPIN_INTERVAL) {
            source.poke();
        }
        core::hint::spin_loop();
    }
    done()
}

/// Prove real interrupt delivery, then prove the counter is driven by it.
///
/// # Safety
///
/// CPL0, after the vector is installed in the live IDT and the source is
/// programmed but masked. Returns with interrupts disabled and the source
/// masked, whatever the outcome.
pub unsafe fn run(required_ticks: u64, source: &impl InterruptSource) -> DeliveryProof {
    let ticks = || source.ticks();
    let set_masked = |masked| source.set_masked(masked);

    set_masked(false);
    // SAFETY: the caller guarantees the vector is installed and the rest of the
    // kernel is ready to take this interrupt.
    unsafe { enable_interrupts() };

    let delivered = spin_until(source, PROOF_SPIN_BUDGET, || ticks() >= required_ticks);
    let ticks_after_run = ticks();
    if !delivered {
        // SAFETY: CPL0; restores the documented exit state.
        unsafe { disable_interrupts() };
        set_masked(true);
        return DeliveryProof::NotDelivered {
            ticks: ticks_after_run,
        };
    }

    // Negative test: mask the source with IF still set. Real deliveries stop.
    set_masked(true);
    let before = ticks();
    // One in-flight interrupt may already have been accepted when the mask was
    // written, so settle first, then require a strictly frozen window.
    spin_until(source, PROOF_SPIN_BUDGET / 20, || false);
    let settled = ticks();
    spin_until(source, PROOF_SPIN_BUDGET / 20, || false);
    let ticks_while_masked = ticks();
    if ticks_while_masked != settled {
        // SAFETY: CPL0.
        unsafe { disable_interrupts() };
        return DeliveryProof::MaskIneffective {
            before,
            after: ticks_while_masked,
        };
    }

    // Unmask and require delivery to resume.
    set_masked(false);
    let resumed = spin_until(source, PROOF_SPIN_BUDGET, || ticks() > ticks_while_masked);
    let ticks_after_unmask = ticks();

    // SAFETY: CPL0; restores the documented exit state.
    unsafe { disable_interrupts() };
    set_masked(true);

    if !resumed {
        return DeliveryProof::DidNotResume {
            ticks: ticks_after_unmask,
        };
    }

    DeliveryProof::Passed {
        ticks_after_run,
        ticks_while_masked,
        ticks_after_unmask,
    }
}
