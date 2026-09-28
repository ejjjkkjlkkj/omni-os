//! Device interrupts routed through an I/O APIC, proved by real delivery.
//!
//! The local APIC timer proved that the CPU can take an interrupt from its own
//! local APIC. That says nothing about the path an actual device uses: an
//! interrupt pin, an I/O APIC redirection entry, and the global system
//! interrupt number the MADT says that pin really carries. This module drives
//! that whole path with the one device every x86 platform has - the 8254 - and
//! holds it to [`crate::irq_proof`]'s bar, mask test included.

use core::sync::atomic::{AtomicU64, Ordering};

use aw_acpi::{Madt, Polarity, TriggerMode};
use aw_x86_interrupts::ioapic::{
    DeliveryMode, DestinationMode, PinPolarity, PinTrigger, RedirectionEntry,
};

use crate::interrupt_vectors;
use crate::ioapic::{IoApic, IoApicError};
use crate::irq_proof::{self, DeliveryProof};
use crate::local_apic::{X2APIC_ID_MSR, x2apic_eoi, x2apic_read};
use crate::pit;

/// ISA IRQ 0: the 8254's channel 0 output.
pub const PIT_ISA_IRQ: u8 = 0;

/// Rate the proof runs the PIT at. Fast enough that several deliveries happen
/// promptly under TCG, slow enough that the handler always returns before the
/// next edge.
pub const PIT_PROOF_HZ: u32 = 1_000;

/// Deliveries required before the proof accepts that routing works. More than
/// one, so a single spurious entry cannot pass it.
pub const REQUIRED_TICKS: u64 = 8;

static IOAPIC_TICKS: AtomicU64 = AtomicU64::new(0);

crate::device_interrupt_stub!(aw_ioapic_isr, ioapic_dispatch);

extern "C" fn ioapic_dispatch() {
    IOAPIC_TICKS.fetch_add(1, Ordering::Relaxed);
    // Attribute the interrupt to the CPU that ran this ISR, reached through `GS`
    // (dossier section 8). Uncounted on any CPU without a per-CPU block yet.
    crate::percpu::count_device_tick();
    // SAFETY: CPL0 interrupt context on a CPU whose x2APIC is enabled. EOI must
    // be signalled before `iretq` or the local APIC keeps this priority level
    // blocked and no further interrupt at this level is delivered.
    unsafe { x2apic_eoi() };
}

#[must_use]
pub fn ioapic_ticks() -> u64 {
    IOAPIC_TICKS.load(Ordering::Acquire)
}

/// What the MADT said and what the kernel programmed because of it.
#[derive(Clone, Copy, Debug)]
pub struct IoApicRouting {
    /// The identifier the I/O APIC itself reports.
    pub io_apic_id: u8,
    /// The identifier the MADT claimed for it. Reported separately because a
    /// disagreement between the two is a firmware description bug worth seeing.
    pub madt_id: u8,
    pub base: u64,
    pub entry_count: u32,
    pub global_system_interrupt: u32,
    pub redirection_index: u32,
    pub vector: u8,
}

/// A programmed, still-masked route from a device pin to a CPU vector.
pub struct RoutedIrq {
    io_apic: IoApic,
    pub routing: IoApicRouting,
}

/// ACPI describes the electrical configuration relative to the bus. For the ISA
/// bus "conforms to bus specification" means active high and edge triggered.
const fn isa_polarity(polarity: Polarity) -> PinPolarity {
    match polarity {
        Polarity::ActiveLow => PinPolarity::ActiveLow,
        Polarity::ActiveHigh | Polarity::ConformsToBus => PinPolarity::ActiveHigh,
    }
}

const fn isa_trigger(trigger: TriggerMode) -> PinTrigger {
    match trigger {
        TriggerMode::Level => PinTrigger::Level,
        TriggerMode::Edge | TriggerMode::ConformsToBus => PinTrigger::Edge,
    }
}

/// Route one ISA IRQ through the I/O APIC the MADT names, to a caller-supplied
/// interrupt entry stub, leaving the redirection entry masked.
///
/// Nothing here is derived from convention: the global system interrupt, the
/// I/O APIC that owns it, and the electrical configuration all come from the
/// MADT, and a route that cannot be built from it is refused by name instead
/// of falling back to the conventional numbers. `stub_addr` is the address of a
/// bare interrupt stub (see [`crate::device_interrupt_stub!`]) that EOIs the
/// local APIC; the PIT proof and the keyboard driver each pass their own.
///
/// # Safety
///
/// CPL0, single core, after the IDT is installed and the local APIC has been
/// enabled in x2APIC mode. `stub_addr` must name a valid interrupt entry stub.
pub unsafe fn route_isa_irq(
    madt: Madt<'static>,
    isa_irq: u8,
    stub_addr: u64,
) -> Result<(IoApic, IoApicRouting), &'static str> {
    // SAFETY: CPL0; reading the APIC base MSR has no side effects.
    let apic_base = unsafe { crate::local_apic::read_apic_base() };
    if !apic_base.enabled || !apic_base.x2apic_enabled {
        return Err("x2apic_not_enabled");
    }

    // SAFETY: CPL0, x2APIC confirmed enabled above.
    let apic_id = unsafe { x2apic_read(X2APIC_ID_MSR) };
    if apic_id > u32::from(u8::MAX) {
        // An I/O APIC redirection entry carries an 8-bit destination. Reaching
        // a wider APIC ID needs interrupt remapping, which this kernel does not
        // program yet, so refuse rather than truncate to the wrong CPU.
        return Err("apic_id_too_wide_for_ioapic");
    }

    let (gsi, polarity, trigger) = madt.resolve_isa_irq(isa_irq);
    let Some((madt_id, address, redirection_index)) = madt.io_apic_for_gsi(gsi) else {
        return Err("no_ioapic_for_gsi");
    };

    // SAFETY: the address comes from a validated MADT I/O APIC entry, and the
    // kernel's identity map is active. Single core, so nothing else is using
    // the selector/window pair.
    let io_apic = unsafe { IoApic::new(u64::from(address)) }.map_err(IoApicError::name)?;

    let Some(vector) = interrupt_vectors::allocate_device_vector() else {
        return Err("no_device_vector");
    };

    let entry = RedirectionEntry::new(
        vector,
        DeliveryMode::Fixed,
        DestinationMode::Physical,
        isa_polarity(polarity),
        isa_trigger(trigger),
        true,
        apic_id as u8,
    )
    .map_err(|_| "redirection_entry_encoding")?;

    // SAFETY: CPL0 with interrupts disabled; the gate uses the same audited
    // encoder as every other vector and the stub preserves all registers.
    unsafe { crate::interrupts::install_interrupt_gate(vector, stub_addr, 0) }
        .map_err(|_| "idt_gate_encoding")?;

    // SAFETY: single core, CPL0; the entry is written masked.
    unsafe { io_apic.write_entry(redirection_index, entry) }.map_err(IoApicError::name)?;

    Ok((
        io_apic,
        IoApicRouting {
            // SAFETY: single core, CPL0; exclusive use of the selector/window.
            io_apic_id: unsafe { io_apic.id() },
            madt_id,
            base: io_apic.base(),
            entry_count: io_apic.entry_count(),
            global_system_interrupt: gsi,
            redirection_index,
            vector,
        },
    ))
}

/// Route ISA IRQ 0 (the 8254) through the I/O APIC, leaving it masked.
///
/// # Safety
///
/// CPL0, single core, after the IDT is installed and the local APIC has been
/// enabled in x2APIC mode.
pub unsafe fn route_pit_through_ioapic(madt: Madt<'static>) -> Result<RoutedIrq, &'static str> {
    // SAFETY: CPL0, x2APIC enabled; delegates to the shared ISA-IRQ router with
    // the PIT's own interrupt stub.
    let (io_apic, routing) =
        unsafe { route_isa_irq(madt, PIT_ISA_IRQ, aw_ioapic_isr as *const () as u64) }?;
    Ok(RoutedIrq { io_apic, routing })
}

/// Start the PIT and require the routed interrupt to actually arrive.
///
/// # Safety
///
/// CPL0, single core, on a route from [`route_pit_through_ioapic`]. The legacy
/// PIC must already be masked, otherwise the same PIT edge is also delivered
/// through the 8259 and the proof could not attribute what it counted. Returns
/// with interrupts disabled and the redirection entry masked.
pub unsafe fn prove_routed_delivery(routed: &RoutedIrq) -> DeliveryProof {
    // Start the source only once the route is programmed and masked, so no edge
    // can arrive before the vector exists.
    // SAFETY: CPL0; ISA IRQ 0 is routed and masked at the I/O APIC.
    unsafe { pit::program_rate_generator(pit::divisor_for_hz(PIT_PROOF_HZ)) };

    // SAFETY: CPL0 after the vector is installed and the entry programmed.
    unsafe { irq_proof::run(REQUIRED_TICKS, routed) }
}

impl irq_proof::InterruptSource for RoutedIrq {
    fn ticks(&self) -> u64 {
        ioapic_ticks()
    }

    fn set_masked(&self, masked: bool) {
        // SAFETY: CPL0, single core; this touches only the mask bit of the
        // entry this route owns. A failed write must not be papered over: an
        // unmask that fails simply stops delivery, and the proof then fails
        // honestly instead of reporting a route that was never live.
        let _ = unsafe {
            self.io_apic
                .set_masked(self.routing.redirection_index, masked)
        };
    }
}
