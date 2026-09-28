//! PCI configuration space through the PCIe ECAM window.
//!
//! Every access is bounds-checked against the ECAM region the firmware
//! described in ACPI MCFG and the loader passed across in the handoff: a
//! configuration address is just an offset into a memory window, so an
//! unchecked bus/device/function triple would read or write whatever happens to
//! follow that window.
#![allow(dead_code)]

use aw_kernel_core::PciEcamHandoff;

/// Offset of the command register, whose memory-space and bus-master enables
/// gate MMIO access and device-initiated writes (MSI included).
pub const COMMAND_REGISTER: u16 = 0x04;
pub const COMMAND_MEMORY_SPACE: u16 = 1 << 1;
pub const COMMAND_BUS_MASTER: u16 = 1 << 2;
/// Offset of the status register; bit 4 says a capability list exists.
pub const STATUS_REGISTER: u16 = 0x06;
pub const STATUS_CAPABILITY_LIST: u16 = 1 << 4;
/// Offset of the pointer to the first capability structure.
pub const CAPABILITY_POINTER: u16 = 0x34;
/// Offset of the first base address register.
pub const BAR0: u16 = 0x10;

/// One addressable PCI function inside an ECAM region.
#[derive(Clone, Copy, Debug)]
pub struct PciFunction {
    region: PciEcamHandoff,
    bus: u8,
    device: u8,
    function: u8,
}

impl PciFunction {
    #[must_use]
    pub fn new(region: PciEcamHandoff, bus: u8, device: u8, function: u8) -> Option<Self> {
        if !region.is_valid() || bus < region.start_bus || bus > region.end_bus {
            return None;
        }
        if device > 31 || function > 7 {
            return None;
        }
        Some(Self {
            region,
            bus,
            device,
            function,
        })
    }

    #[must_use]
    pub fn bus(self) -> u8 {
        self.bus
    }

    #[must_use]
    pub fn device(self) -> u8 {
        self.device
    }

    #[must_use]
    pub fn function(self) -> u8 {
        self.function
    }

    #[must_use]
    pub fn read_u32(self, register_offset: u16) -> Option<u32> {
        read_u32(
            self.region,
            self.bus,
            self.device,
            self.function,
            register_offset,
        )
    }

    /// Write one configuration dword.
    ///
    /// # Safety
    ///
    /// Configuration writes reprogram the device. The caller must own this
    /// function and must not write registers firmware is still relying on -
    /// notably a BAR, which would move a window the loader already mapped.
    pub unsafe fn write_u32(self, register_offset: u16, value: u32) -> bool {
        // SAFETY: delegated to the caller, plus the bounds check inside.
        unsafe {
            write_u32(
                self.region,
                self.bus,
                self.device,
                self.function,
                register_offset,
                value,
            )
        }
    }

    #[must_use]
    pub fn read_u16(self, register_offset: u16) -> Option<u16> {
        let aligned = register_offset & !3;
        let shift = (register_offset & 2) * 8;
        Some((self.read_u32(aligned)? >> shift) as u16)
    }

    /// Read-modify-write of one 16-bit configuration field.
    ///
    /// # Safety
    /// As [`Self::write_u32`].
    pub unsafe fn write_u16(self, register_offset: u16, value: u16) -> bool {
        let aligned = register_offset & !3;
        let shift = (register_offset & 2) * 8;
        let Some(current) = self.read_u32(aligned) else {
            return false;
        };
        let updated = (current & !(0xffff << shift)) | (u32::from(value) << shift);
        // SAFETY: delegated to the caller.
        unsafe { self.write_u32(aligned, updated) }
    }

    #[must_use]
    pub fn read_u8(self, register_offset: u16) -> Option<u8> {
        let aligned = register_offset & !3;
        let shift = (register_offset & 3) * 8;
        Some((self.read_u32(aligned)? >> shift) as u8)
    }
}

/// Physical address of one configuration dword inside an ECAM region.
#[must_use]
pub fn ecam_address(
    region: PciEcamHandoff,
    bus: u8,
    device: u8,
    function: u8,
    register_offset: u16,
) -> Option<u64> {
    if !region.is_valid()
        || bus < region.start_bus
        || bus > region.end_bus
        || device > 31
        || function > 7
        || register_offset > 0x0ffc
        || register_offset & 3 != 0
    {
        return None;
    }

    let relative_bus = u64::from(bus - region.start_bus);
    let offset = (relative_bus << 20)
        | (u64::from(device) << 15)
        | (u64::from(function) << 12)
        | u64::from(register_offset);
    let address = region.base_address.checked_add(offset)?;
    if address > usize::MAX as u64 {
        return None;
    }
    Some(address)
}

#[must_use]
pub fn read_u32(
    region: PciEcamHandoff,
    bus: u8,
    device: u8,
    function: u8,
    register_offset: u16,
) -> Option<u32> {
    let address = ecam_address(region, bus, device, function, register_offset)?;
    // SAFETY: The region was validated from ACPI MCFG before ExitBootServices
    // and the offset is bounds-checked above. ECAM configuration registers are
    // MMIO and are read using volatile access.
    Some(unsafe { core::ptr::read_volatile(address as usize as *const u32) })
}

/// # Safety
///
/// See [`PciFunction::write_u32`].
pub unsafe fn write_u32(
    region: PciEcamHandoff,
    bus: u8,
    device: u8,
    function: u8,
    register_offset: u16,
    value: u32,
) -> bool {
    let Some(address) = ecam_address(region, bus, device, function, register_offset) else {
        return false;
    };
    // SAFETY: bounds-checked ECAM address; the caller owns this function.
    unsafe { core::ptr::write_volatile(address as usize as *mut u32, value) };
    true
}
