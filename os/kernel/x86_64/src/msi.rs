//! The PCI MSI capability: where a device is told which interrupt to send.
//!
//! An MSI is not a wire. The device performs an ordinary posted memory write
//! into the local APIC's message window, with the target CPU encoded in the
//! address and the vector in the data. So "routing" a device here means writing
//! two words into its configuration space - and nothing arrives unless the
//! device is also allowed to write to memory at all, which is why enabling bus
//! mastering is part of this path rather than an afterthought.
//!
//! Message encoding lives in [`aw_x86_interrupts::msi`], which is host-tested
//! and touches no hardware. This module performs the configuration accesses.
#![allow(dead_code)]

use aw_x86_interrupts::msi::MsiMessage;

use crate::pci_config::{
    CAPABILITY_POINTER, COMMAND_BUS_MASTER, COMMAND_MEMORY_SPACE, COMMAND_REGISTER, PciFunction,
    STATUS_CAPABILITY_LIST, STATUS_REGISTER,
};

/// PCI capability ID for MSI.
const CAPABILITY_ID_MSI: u8 = 0x05;

/// A capability list is a chain in a 256-byte space, so it cannot legitimately
/// be longer than this. The bound is what stops a corrupt or hostile chain from
/// looping forever.
const MAX_CAPABILITIES: usize = 48;

/// The lowest offset a capability structure may live at; everything below is
/// the architected configuration header.
const FIRST_CAPABILITY_OFFSET: u8 = 0x40;

const MESSAGE_CONTROL: u16 = 0x02;
const MESSAGE_ADDRESS: u16 = 0x04;

const CONTROL_ENABLE: u16 = 1 << 0;
const CONTROL_MULTIPLE_MESSAGE_ENABLE: u16 = 0b111 << 4;
const CONTROL_ADDRESS_64: u16 = 1 << 7;

/// An MSI capability located in one function's configuration space.
#[derive(Clone, Copy, Debug)]
pub struct MsiCapability {
    function: PciFunction,
    offset: u16,
    address_64: bool,
}

impl MsiCapability {
    /// Walk the capability list and return the MSI capability, if any.
    #[must_use]
    pub fn find(function: PciFunction) -> Option<Self> {
        let status = function.read_u16(STATUS_REGISTER)?;
        if status & STATUS_CAPABILITY_LIST == 0 {
            return None;
        }

        let mut offset = function.read_u8(CAPABILITY_POINTER)? & !0b11;
        for _ in 0..MAX_CAPABILITIES {
            if offset < FIRST_CAPABILITY_OFFSET {
                return None;
            }

            let header = function.read_u16(u16::from(offset))?;
            let id = header as u8;
            let next = (header >> 8) as u8 & !0b11;

            if id == CAPABILITY_ID_MSI {
                let control = function.read_u16(u16::from(offset) + MESSAGE_CONTROL)?;
                return Some(Self {
                    function,
                    offset: u16::from(offset),
                    address_64: control & CONTROL_ADDRESS_64 != 0,
                });
            }

            if next == 0 {
                return None;
            }
            offset = next;
        }

        None
    }

    /// Offset of the 16-bit message data register, which moves depending on
    /// whether the capability carries a 64-bit address.
    const fn data_offset(self) -> u16 {
        if self.address_64 {
            self.offset + 0x0c
        } else {
            self.offset + 0x08
        }
    }

    #[must_use]
    pub fn supports_64_bit_address(self) -> bool {
        self.address_64
    }

    #[must_use]
    pub fn is_enabled(self) -> bool {
        self.function
            .read_u16(self.offset + MESSAGE_CONTROL)
            .is_some_and(|control| control & CONTROL_ENABLE != 0)
    }

    /// Write the message the device must send, leaving MSI disabled.
    ///
    /// Exactly one vector is requested: the multiple-message-enable field is
    /// cleared, so the device may not derive additional vectors by varying the
    /// low bits of the data word.
    ///
    /// # Safety
    ///
    /// The caller must own this function, and `message` must name a vector
    /// already installed in the live IDT.
    pub unsafe fn program(self, message: MsiMessage) -> bool {
        // SAFETY: the caller owns this function; these are the capability's own
        // registers, not a BAR or anything firmware still depends on.
        unsafe {
            if !self.function.write_u32(self.offset + MESSAGE_ADDRESS, message.address) {
                return false;
            }
            if self.address_64
                && !self
                    .function
                    .write_u32(self.offset + 0x08, message.address_high())
            {
                return false;
            }
            if !self
                .function
                .write_u16(self.data_offset(), message.data as u16)
            {
                return false;
            }
        }

        let Some(control) = self.function.read_u16(self.offset + MESSAGE_CONTROL) else {
            return false;
        };
        let single_vector = control & !CONTROL_MULTIPLE_MESSAGE_ENABLE & !CONTROL_ENABLE;
        // SAFETY: as above.
        unsafe {
            self.function
                .write_u16(self.offset + MESSAGE_CONTROL, single_vector)
        }
    }

    /// Set or clear the capability's enable bit.
    ///
    /// This is the device's own mask: with it clear the device sends nothing,
    /// whatever the interrupt controllers upstream are doing.
    ///
    /// # Safety
    /// The caller must own this function.
    pub unsafe fn set_enabled(self, enabled: bool) -> bool {
        let Some(control) = self.function.read_u16(self.offset + MESSAGE_CONTROL) else {
            return false;
        };
        let updated = if enabled {
            control | CONTROL_ENABLE
        } else {
            control & !CONTROL_ENABLE
        };
        // SAFETY: the caller owns this function; only the enable bit changes.
        unsafe {
            self.function
                .write_u16(self.offset + MESSAGE_CONTROL, updated)
        }
    }
}

/// Allow a function to decode MMIO and to initiate memory writes of its own.
///
/// Bus mastering is not optional for MSI: the interrupt *is* a write the device
/// performs, so a device without it is silent no matter how its capability is
/// programmed.
///
/// # Safety
/// The caller must own this function.
pub unsafe fn enable_memory_and_bus_master(function: PciFunction) -> bool {
    let Some(command) = function.read_u16(COMMAND_REGISTER) else {
        return false;
    };
    // SAFETY: the caller owns this function; only enable bits are set, and
    // nothing that would move a window firmware already mapped.
    unsafe {
        function.write_u16(
            COMMAND_REGISTER,
            command | COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER,
        )
    }
}
