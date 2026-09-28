//! I/O APIC MMIO access.
//!
//! The I/O APIC exposes exactly two 32-bit locations: a register selector and
//! a data window. Every register is reached by writing its index to the
//! selector and then reading or writing the window, which makes the pair
//! stateful and strictly non-reentrant - two interleaved accesses would read
//! each other's register.
//!
//! Entry encoding lives in [`aw_x86_interrupts::ioapic`], which is host-tested
//! and performs no access. This module performs the access and nothing else.

use aw_x86_interrupts::ioapic::{
    IOAPIC_REGISTER_ID, IOAPIC_REGISTER_VERSION, IOAPIC_REGSEL_OFFSET, IOAPIC_WINDOW_OFFSET,
    RedirectionEntry, RedirectionEntryError, redirection_entry_count, redirection_registers,
};

use crate::virtual_memory::IDENTITY_GIB;

const IDENTITY_LIMIT: u64 = IDENTITY_GIB << 30;
/// The register selector and window occupy the first 0x14 bytes of the block.
const IOAPIC_MMIO_LEN: u64 = 0x14;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoApicError {
    /// The base address is null, misaligned, or outside the identity map.
    Unreachable,
    /// The redirection entry index is past what this I/O APIC implements.
    EntryOutOfRange,
    Encoding(RedirectionEntryError),
}

impl IoApicError {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Unreachable => "ioapic_unreachable",
            Self::EntryOutOfRange => "ioapic_entry_out_of_range",
            Self::Encoding(_) => "ioapic_entry_encoding",
        }
    }
}

/// One I/O APIC, located from the MADT.
#[derive(Clone, Copy, Debug)]
pub struct IoApic {
    base: u64,
    entry_count: u32,
}

impl IoApic {
    /// Map an I/O APIC by its firmware-reported physical base address and read
    /// back how many redirection entries it implements.
    ///
    /// # Safety
    ///
    /// `base` must be the address of a real I/O APIC as reported by the MADT,
    /// and the kernel's identity map must already be active. Nothing else may
    /// be touching this I/O APIC concurrently.
    pub unsafe fn new(base: u64) -> Result<Self, IoApicError> {
        if base == 0 || !base.is_multiple_of(16) {
            return Err(IoApicError::Unreachable);
        }
        if base.checked_add(IOAPIC_MMIO_LEN).ok_or(IoApicError::Unreachable)? > IDENTITY_LIMIT {
            return Err(IoApicError::Unreachable);
        }

        let mut io_apic = Self {
            base,
            entry_count: 0,
        };
        // SAFETY: the base has been bounds-checked and the caller guarantees it
        // designates an I/O APIC.
        io_apic.entry_count = redirection_entry_count(unsafe { io_apic.read(IOAPIC_REGISTER_VERSION) });
        Ok(io_apic)
    }

    /// # Safety
    /// Requires exclusive use of the selector/window pair.
    unsafe fn read(&self, register: u8) -> u32 {
        // SAFETY: both locations are inside the bounds-checked MMIO block, and
        // the selector write must be visible before the window read, which
        // volatile ordering on the same device guarantees.
        unsafe {
            core::ptr::write_volatile(
                (self.base + IOAPIC_REGSEL_OFFSET) as usize as *mut u32,
                u32::from(register),
            );
            core::ptr::read_volatile((self.base + IOAPIC_WINDOW_OFFSET) as usize as *const u32)
        }
    }

    /// # Safety
    /// Requires exclusive use of the selector/window pair.
    unsafe fn write(&self, register: u8, value: u32) {
        // SAFETY: as above, for the write direction.
        unsafe {
            core::ptr::write_volatile(
                (self.base + IOAPIC_REGSEL_OFFSET) as usize as *mut u32,
                u32::from(register),
            );
            core::ptr::write_volatile(
                (self.base + IOAPIC_WINDOW_OFFSET) as usize as *mut u32,
                value,
            );
        }
    }

    #[must_use]
    pub fn base(&self) -> u64 {
        self.base
    }

    #[must_use]
    pub fn entry_count(&self) -> u32 {
        self.entry_count
    }

    /// The 4-bit identifier firmware programmed into this I/O APIC.
    ///
    /// # Safety
    /// Requires exclusive use of the selector/window pair.
    pub unsafe fn id(&self) -> u8 {
        // SAFETY: delegated to `read`.
        ((unsafe { self.read(IOAPIC_REGISTER_ID) } >> 24) & 0x0f) as u8
    }

    fn check_index(&self, index: u32) -> Result<u8, IoApicError> {
        if index >= self.entry_count || index > u32::from(u8::MAX) {
            return Err(IoApicError::EntryOutOfRange);
        }
        Ok(index as u8)
    }

    /// Write a complete redirection entry.
    ///
    /// The entry is masked first, then the destination half is written, then
    /// the low half. Writing the low half last means the interrupt is never
    /// briefly unmasked while the destination still holds the previous value.
    ///
    /// # Safety
    /// CPL0, with exclusive use of this I/O APIC.
    pub unsafe fn write_entry(&self, index: u32, entry: RedirectionEntry) -> Result<(), IoApicError> {
        let index = self.check_index(index)?;
        let (low, high) = redirection_registers(index).map_err(IoApicError::Encoding)?;

        // SAFETY: registers derived from a bounds-checked entry index.
        unsafe {
            self.write(low, entry.with_mask(true).low());
            self.write(high, entry.high());
            self.write(low, entry.low());
        }
        Ok(())
    }

    /// Read a redirection entry back out of the hardware.
    ///
    /// # Safety
    /// CPL0, with exclusive use of this I/O APIC.
    pub unsafe fn read_entry(&self, index: u32) -> Result<RedirectionEntry, IoApicError> {
        let index = self.check_index(index)?;
        let (low, high) = redirection_registers(index).map_err(IoApicError::Encoding)?;
        // SAFETY: registers derived from a bounds-checked entry index.
        Ok(unsafe { RedirectionEntry::from_halves(self.read(low), self.read(high)) })
    }

    /// Set or clear one entry's mask bit, leaving its routing untouched.
    ///
    /// # Safety
    /// CPL0, with exclusive use of this I/O APIC.
    pub unsafe fn set_masked(&self, index: u32, masked: bool) -> Result<(), IoApicError> {
        let checked = self.check_index(index)?;
        let (low, _) = redirection_registers(checked).map_err(IoApicError::Encoding)?;
        // SAFETY: read-modify-write of the half holding the mask bit, on a
        // bounds-checked register index.
        unsafe {
            let current = self.read_entry(index)?;
            self.write(low, current.with_mask(masked).low());
        }
        Ok(())
    }
}
