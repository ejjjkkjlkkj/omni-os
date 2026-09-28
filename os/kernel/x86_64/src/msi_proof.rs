//! MSI delivery proof, driven by QEMU's `edu` test device.
//!
//! This module is behind the `msi-proof-device` feature and is never part of a
//! normal image: it contains a driver for a device that exists only inside an
//! emulator. What it proves, though, is not emulator-specific - the capability
//! walk, the message programming, the bus-master enable and the vector plumbing
//! in [`crate::msi`] are exactly what a real NVMe or xHCI controller will use.
//! `edu` is here because it is the one device that can be asked to raise an
//! interrupt without first implementing its whole command protocol.
//!
//! The proof is the same one every interrupt source in this kernel has to pass,
//! with the negative test made stronger by the device's nature: the source is
//! poked continuously throughout the masked window, so a frozen counter there
//! means the device's own MSI enable bit really suppressed interrupts that were
//! actively being requested.

use core::sync::atomic::{AtomicU64, Ordering};

use aw_kernel_core::{HANDOFF_FLAG_PCIE_ECAM_PRESENT, KernelHandoff};
use aw_x86_interrupts::ioapic::{DeliveryMode, PinTrigger};
use aw_x86_interrupts::msi::MsiMessage;

use crate::interrupt_vectors;
use crate::irq_proof::{self, DeliveryProof, InterruptSource};
use crate::local_apic::{X2APIC_ID_MSR, x2apic_eoi, x2apic_read};
use crate::msi::{MsiCapability, enable_memory_and_bus_master};
use crate::pci_config::{BAR0, PciFunction};
use crate::virtual_memory::IDENTITY_GIB;

const IDENTITY_LIMIT: u64 = IDENTITY_GIB << 30;

/// QEMU's educational PCI device.
const EDU_VENDOR_ID: u16 = 0x1234;
const EDU_DEVICE_ID: u16 = 0x11e8;
/// Value the device's identification register returns, used to confirm that
/// BAR0 really is decoding before anything is asked of the device.
const EDU_IDENTIFICATION_MASK: u32 = 0xffff_0000;
const EDU_IDENTIFICATION: u32 = 0x0100_0000;

const EDU_REG_IDENTIFICATION: u64 = 0x00;
const EDU_REG_IRQ_RAISE: u64 = 0x60;
const EDU_REG_IRQ_ACK: u64 = 0x64;
/// The single interrupt-status bit this proof raises and acknowledges.
const EDU_IRQ_BIT: u32 = 1;

/// Deliveries required before the proof accepts that MSI works.
pub const REQUIRED_TICKS: u64 = 8;

static MSI_TICKS: AtomicU64 = AtomicU64::new(0);
/// Published so the interrupt handler can acknowledge the device. Written once,
/// before the vector is ever unmasked.
static EDU_MMIO_BASE: AtomicU64 = AtomicU64::new(0);

crate::device_interrupt_stub!(aw_msi_isr, msi_dispatch);

extern "C" fn msi_dispatch() {
    MSI_TICKS.fetch_add(1, Ordering::Relaxed);

    let base = EDU_MMIO_BASE.load(Ordering::Acquire);
    if base != 0 {
        // SAFETY: the base was bounds-checked before being published, and the
        // device is acknowledged before EOI so it can raise the next message.
        unsafe {
            core::ptr::write_volatile((base + EDU_REG_IRQ_ACK) as usize as *mut u32, EDU_IRQ_BIT);
        }
    }

    // SAFETY: CPL0 interrupt context on a CPU whose x2APIC is enabled.
    unsafe { x2apic_eoi() };
}

#[must_use]
pub fn msi_ticks() -> u64 {
    MSI_TICKS.load(Ordering::Acquire)
}

/// A located, programmed MSI source, still disabled at the device.
pub struct MsiProofDevice {
    capability: MsiCapability,
    mmio_base: u64,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vector: u8,
    pub message: MsiMessage,
}

impl InterruptSource for MsiProofDevice {
    fn ticks(&self) -> u64 {
        msi_ticks()
    }

    fn set_masked(&self, masked: bool) {
        // SAFETY: this kernel owns the device; only the capability's enable bit
        // changes. A failed write leaves the device disabled, which fails the
        // proof honestly rather than reporting delivery that never happened.
        let _ = unsafe { self.capability.set_enabled(!masked) };
    }

    fn poke(&self) {
        // SAFETY: bounds-checked MMIO inside the identity map, on a device this
        // kernel owns. Raising is idempotent: the device ORs the bit in.
        unsafe {
            core::ptr::write_volatile(
                (self.mmio_base + EDU_REG_IRQ_RAISE) as usize as *mut u32,
                EDU_IRQ_BIT,
            );
        }
    }
}

/// Decode a 32-bit or 64-bit memory BAR into a physical base address.
fn memory_bar_base(function: PciFunction) -> Option<u64> {
    let low = function.read_u32(BAR0)?;
    // Bit 0 set means an I/O BAR, which cannot be memory-mapped.
    if low & 1 != 0 {
        return None;
    }

    let base = u64::from(low & !0xf);
    let base = if (low >> 1) & 0b11 == 0b10 {
        let high = function.read_u32(BAR0 + 4)?;
        base | (u64::from(high) << 32)
    } else {
        base
    };

    if base == 0 || base >= IDENTITY_LIMIT {
        return None;
    }
    Some(base)
}

fn find_proof_device(handoff: &KernelHandoff) -> Option<PciFunction> {
    if handoff.flags & HANDOFF_FLAG_PCIE_ECAM_PRESENT == 0 {
        return None;
    }

    for region in handoff
        .pcie_ecam
        .iter()
        .take(handoff.pcie_ecam_count as usize)
        .copied()
    {
        for bus in region.start_bus..=region.end_bus {
            for device in 0_u8..32 {
                for slot in 0_u8..8 {
                    let Some(function) = PciFunction::new(region, bus, device, slot) else {
                        continue;
                    };
                    let Some(identity) = function.read_u32(0x00) else {
                        continue;
                    };
                    if identity as u16 == EDU_VENDOR_ID
                        && (identity >> 16) as u16 == EDU_DEVICE_ID
                    {
                        return Some(function);
                    }
                }
            }
        }
    }

    None
}

/// Locate the proof device, program its MSI capability, and leave it disabled.
///
/// # Safety
///
/// CPL0, single core, after the IDT is installed and the local APIC is enabled
/// in x2APIC mode.
pub unsafe fn program_msi_device(handoff: &KernelHandoff) -> Result<MsiProofDevice, &'static str> {
    // SAFETY: CPL0; reading the APIC base MSR has no side effects.
    let apic_base = unsafe { crate::local_apic::read_apic_base() };
    if !apic_base.enabled || !apic_base.x2apic_enabled {
        return Err("x2apic_not_enabled");
    }
    // SAFETY: CPL0, x2APIC confirmed enabled above.
    let apic_id = unsafe { x2apic_read(X2APIC_ID_MSR) };
    if apic_id > u32::from(u8::MAX) {
        // A physically addressed MSI carries an 8-bit destination APIC ID.
        return Err("apic_id_too_wide_for_msi");
    }

    let function = find_proof_device(handoff).ok_or("no_proof_device")?;
    let capability = MsiCapability::find(function).ok_or("no_msi_capability")?;

    // SAFETY: this kernel owns the device; the write only sets enable bits.
    if !unsafe { enable_memory_and_bus_master(function) } {
        return Err("command_register_write");
    }

    let mmio_base = memory_bar_base(function).ok_or("unusable_bar0")?;
    // SAFETY: the base is inside the identity map and memory decoding was just
    // enabled, so the device answers this read.
    let identification =
        unsafe { core::ptr::read_volatile((mmio_base + EDU_REG_IDENTIFICATION) as *const u32) };
    if identification & EDU_IDENTIFICATION_MASK != EDU_IDENTIFICATION {
        return Err("bar0_not_decoding");
    }

    let vector = interrupt_vectors::allocate_device_vector().ok_or("no_device_vector")?;
    let message = MsiMessage::physical(apic_id as u8, vector, DeliveryMode::Fixed, PinTrigger::Edge)
        .map_err(|_| "message_encoding")?;

    // SAFETY: CPL0 with interrupts disabled; the stub preserves all registers
    // and the gate uses the same audited encoder as every other vector.
    unsafe { crate::interrupts::install_interrupt_gate(vector, aw_msi_isr as *const () as u64, 0) }
        .map_err(|_| "idt_gate_encoding")?;

    // The handler can only acknowledge the device once it knows where it is,
    // and the vector must not be enabled before that is true.
    EDU_MMIO_BASE.store(mmio_base, Ordering::Release);

    // SAFETY: this kernel owns the device and the vector is now installed.
    if !unsafe { capability.program(message) } {
        return Err("capability_write");
    }

    Ok(MsiProofDevice {
        capability,
        mmio_base,
        bus: function.bus(),
        device: function.device(),
        function: function.function(),
        vector,
        message,
    })
}

/// Require the programmed MSI to actually arrive.
///
/// # Safety
///
/// CPL0, single core, on a device from [`program_msi_device`]. Returns with
/// interrupts disabled and the device's MSI enable bit clear.
pub unsafe fn prove_msi_delivery(device: &MsiProofDevice) -> DeliveryProof {
    // SAFETY: CPL0 after the vector is installed and the capability programmed.
    unsafe { irq_proof::run(REQUIRED_TICKS, device) }
}
