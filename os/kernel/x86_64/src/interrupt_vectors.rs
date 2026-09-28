#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, Ordering};

/// CPU exceptions remain in Intel-defined vectors 0x00..=0x1F.
pub const CPU_EXCEPTION_FIRST: u8 = 0x00;
pub const CPU_EXCEPTION_LAST: u8 = 0x1F;

/// 0x20..=0x3F are intentionally left available for legacy IRQ/PIC compatibility
/// and early platform bring-up. The Local APIC timer gets its own fixed vector.
pub const APIC_TIMER_VECTOR: u8 = 0x40;

/// Dynamically allocated device/MSI/MSI-X vectors.
pub const DEVICE_VECTOR_FIRST: u8 = 0x50;
pub const DEVICE_VECTOR_LAST: u8 = 0xDF;

/// Reserved IPI range for later SMP work.
pub const IPI_VECTOR_FIRST: u8 = 0xE0;
pub const IPI_VECTOR_LAST: u8 = 0xEF;

/// 0xF0..=0xFD remain reserved for future architecture/platform use.
pub const APIC_ERROR_VECTOR: u8 = 0xFE;
pub const APIC_SPURIOUS_VECTOR: u8 = 0xFF;

static NEXT_DEVICE_VECTOR: AtomicU8 = AtomicU8::new(DEVICE_VECTOR_FIRST);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VectorClass {
    CpuException,
    LegacyReserved,
    ApicTimer,
    Device,
    Ipi,
    ArchitectureReserved,
    ApicError,
    ApicSpurious,
}

pub const fn classify(vector: u8) -> VectorClass {
    match vector {
        CPU_EXCEPTION_FIRST..=CPU_EXCEPTION_LAST => VectorClass::CpuException,
        0x20..=0x3F => VectorClass::LegacyReserved,
        APIC_TIMER_VECTOR => VectorClass::ApicTimer,
        DEVICE_VECTOR_FIRST..=DEVICE_VECTOR_LAST => VectorClass::Device,
        IPI_VECTOR_FIRST..=IPI_VECTOR_LAST => VectorClass::Ipi,
        0xF0..=0xFD => VectorClass::ArchitectureReserved,
        APIC_ERROR_VECTOR => VectorClass::ApicError,
        APIC_SPURIOUS_VECTOR => VectorClass::ApicSpurious,
        _ => VectorClass::ArchitectureReserved,
    }
}

pub fn allocate_device_vector() -> Option<u8> {
    loop {
        let current = NEXT_DEVICE_VECTOR.load(Ordering::Relaxed);
        if current > DEVICE_VECTOR_LAST {
            return None;
        }

        let next = current.saturating_add(1);
        match NEXT_DEVICE_VECTOR.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(current),
            Err(_) => core::hint::spin_loop(),
        }
    }
}

/// Only for deterministic early-boot/self-test scenarios before interrupts are live.
pub fn reset_allocator_for_boot_test() {
    NEXT_DEVICE_VECTOR.store(DEVICE_VECTOR_FIRST, Ordering::Release);
}
