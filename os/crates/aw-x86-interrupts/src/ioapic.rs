//! Pure encoding for I/O APIC redirection-table entries.
//!
//! Like the rest of this crate, nothing here touches hardware: it produces the
//! exact 64-bit value a redirection-table entry must hold and the two register
//! indices it is written through. The kernel performs the MMIO.
//!
//! The encoding is deliberately validated rather than trusted. A redirection
//! entry programmed with a vector inside the CPU exception range would make a
//! device interrupt arrive as, say, a page fault, and the resulting diagnostic
//! would point anywhere but here.

/// `IOAPICID`: the 4-bit I/O APIC identifier, bits 24..27.
pub const IOAPIC_REGISTER_ID: u8 = 0x00;
/// `IOAPICVER`: version in bits 0..7, maximum redirection entry in bits 16..23.
pub const IOAPIC_REGISTER_VERSION: u8 = 0x01;
/// First redirection-table register. Entry `n` occupies `0x10 + 2n` (low half)
/// and `0x11 + 2n` (high half).
pub const IOAPIC_REGISTER_REDIRECTION_BASE: u8 = 0x10;

/// Byte offset of `IOREGSEL` from the I/O APIC's base address.
pub const IOAPIC_REGSEL_OFFSET: u64 = 0x00;
/// Byte offset of the 32-bit data window `IOWIN`.
pub const IOAPIC_WINDOW_OFFSET: u64 = 0x10;

/// Highest redirection entry whose register pair still fits in the 8-bit
/// `IOREGSEL` index.
pub const MAX_REDIRECTION_INDEX: u8 = (u8::MAX - IOAPIC_REGISTER_REDIRECTION_BASE) / 2;

/// How the local APIC is asked to deliver the interrupt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    /// Deliver on the given vector to the listed destination.
    Fixed,
    /// Deliver on the given vector to the lowest-priority listed CPU.
    LowestPriority,
    Smi,
    Nmi,
    Init,
    /// 8259-style delivery, where the vector comes from the external PIC.
    ExtInt,
}

impl DeliveryMode {
    #[must_use]
    pub const fn bits(self) -> u64 {
        match self {
            Self::Fixed => 0b000,
            Self::LowestPriority => 0b001,
            Self::Smi => 0b010,
            Self::Nmi => 0b100,
            Self::Init => 0b101,
            Self::ExtInt => 0b111,
        }
    }

    /// Whether this mode delivers through the vector field at all. SMI, NMI and
    /// INIT ignore it, and the architecture requires it to be zero for them.
    #[must_use]
    pub const fn uses_vector(self) -> bool {
        matches!(self, Self::Fixed | Self::LowestPriority | Self::ExtInt)
    }
}

/// How the destination field is interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DestinationMode {
    /// The destination field is an APIC ID.
    Physical,
    /// The destination field is a logical-destination bitmask.
    Logical,
}

/// Electrical polarity of the interrupt input pin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinPolarity {
    ActiveHigh,
    ActiveLow,
}

/// Trigger mode of the interrupt input pin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinTrigger {
    Edge,
    Level,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectionEntryError {
    /// The vector is inside the CPU exception range, or is the architecturally
    /// reserved 0xff.
    VectorOutOfRange,
    /// A delivery mode that ignores the vector field was given a non-zero one.
    VectorNotAllowedForDeliveryMode,
    /// The entry index does not fit in the 8-bit register selector.
    IndexOutOfRange,
}

/// One 64-bit I/O APIC redirection-table entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedirectionEntry(u64);

impl RedirectionEntry {
    const MASKED: u64 = 1 << 16;

    /// Build an entry, refusing vectors the CPU has already reserved.
    pub const fn new(
        vector: u8,
        delivery_mode: DeliveryMode,
        destination_mode: DestinationMode,
        polarity: PinPolarity,
        trigger: PinTrigger,
        masked: bool,
        destination: u8,
    ) -> Result<Self, RedirectionEntryError> {
        if delivery_mode.uses_vector() {
            if vector < crate::FIRST_USABLE_INTERRUPT_VECTOR || vector == 0xff {
                return Err(RedirectionEntryError::VectorOutOfRange);
            }
        } else if vector != 0 {
            return Err(RedirectionEntryError::VectorNotAllowedForDeliveryMode);
        }

        let destination_mode_bit = match destination_mode {
            DestinationMode::Physical => 0,
            DestinationMode::Logical => 1 << 11,
        };
        let polarity_bit = match polarity {
            PinPolarity::ActiveHigh => 0,
            PinPolarity::ActiveLow => 1 << 13,
        };
        let trigger_bit = match trigger {
            PinTrigger::Edge => 0,
            PinTrigger::Level => 1 << 15,
        };
        let mask_bit = if masked { Self::MASKED } else { 0 };

        Ok(Self(
            vector as u64
                | (delivery_mode.bits() << 8)
                | destination_mode_bit
                | polarity_bit
                | trigger_bit
                | mask_bit
                | ((destination as u64) << 56),
        ))
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Low half, written to register `0x10 + 2n`.
    #[must_use]
    pub const fn low(self) -> u32 {
        self.0 as u32
    }

    /// High half, written to register `0x11 + 2n`. Only the destination field
    /// lives here.
    #[must_use]
    pub const fn high(self) -> u32 {
        (self.0 >> 32) as u32
    }

    #[must_use]
    pub const fn is_masked(self) -> bool {
        self.0 & Self::MASKED != 0
    }

    /// The same entry with its mask bit set or cleared, leaving vector, routing
    /// and electrical configuration untouched.
    #[must_use]
    pub const fn with_mask(self, masked: bool) -> Self {
        if masked {
            Self(self.0 | Self::MASKED)
        } else {
            Self(self.0 & !Self::MASKED)
        }
    }

    /// Reconstruct an entry from the two halves read back out of the hardware.
    ///
    /// Delivery status and remote IRR are read-only status bits the I/O APIC
    /// owns, so a value read back is not necessarily bit-identical to the one
    /// written; this exists for mask manipulation and diagnostics, not for
    /// asserting equality with the written entry.
    #[must_use]
    pub const fn from_halves(low: u32, high: u32) -> Self {
        Self((low as u64) | ((high as u64) << 32))
    }

    #[must_use]
    pub const fn vector(self) -> u8 {
        self.0 as u8
    }

    #[must_use]
    pub const fn destination(self) -> u8 {
        (self.0 >> 56) as u8
    }
}

/// Register indices holding the low and high halves of redirection entry `n`.
pub const fn redirection_registers(index: u8) -> Result<(u8, u8), RedirectionEntryError> {
    if index > MAX_REDIRECTION_INDEX {
        return Err(RedirectionEntryError::IndexOutOfRange);
    }
    let low = IOAPIC_REGISTER_REDIRECTION_BASE + index * 2;
    Ok((low, low + 1))
}

/// Number of redirection entries this I/O APIC implements, decoded from the
/// `IOAPICVER` register's maximum-redirection-entry field.
#[must_use]
pub const fn redirection_entry_count(version_register: u32) -> u32 {
    ((version_register >> 16) & 0xff) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_masked_edge_triggered_fixed_entry() {
        let entry = RedirectionEntry::new(
            0x50,
            DeliveryMode::Fixed,
            DestinationMode::Physical,
            PinPolarity::ActiveHigh,
            PinTrigger::Edge,
            true,
            0,
        )
        .expect("a fixed entry on a device vector is valid");

        assert_eq!(entry.raw(), 0x0000_0000_0001_0050);
        assert!(entry.is_masked());
        assert_eq!(entry.vector(), 0x50);
        assert_eq!(entry.high(), 0);
    }

    #[test]
    fn encodes_level_triggered_active_low_to_a_remote_apic() {
        let entry = RedirectionEntry::new(
            0x51,
            DeliveryMode::LowestPriority,
            DestinationMode::Logical,
            PinPolarity::ActiveLow,
            PinTrigger::Level,
            false,
            0x03,
        )
        .expect("valid entry");

        // vector 0x51, delivery 001, logical, active low, level triggered.
        assert_eq!(entry.low(), 0x0000_a951);
        assert_eq!(entry.high(), 0x0300_0000);
        assert_eq!(entry.destination(), 3);
        assert!(!entry.is_masked());
    }

    #[test]
    fn mask_bit_toggles_without_disturbing_the_rest_of_the_entry() {
        let unmasked = RedirectionEntry::new(
            0x60,
            DeliveryMode::Fixed,
            DestinationMode::Physical,
            PinPolarity::ActiveHigh,
            PinTrigger::Level,
            false,
            1,
        )
        .expect("valid entry");

        let masked = unmasked.with_mask(true);
        assert!(masked.is_masked());
        assert_eq!(masked.with_mask(false), unmasked);
        assert_eq!(masked.raw() & !(1 << 16), unmasked.raw());
    }

    #[test]
    fn rejects_vectors_the_cpu_has_reserved_for_exceptions() {
        for vector in [0_u8, 8, 14, 31] {
            assert_eq!(
                RedirectionEntry::new(
                    vector,
                    DeliveryMode::Fixed,
                    DestinationMode::Physical,
                    PinPolarity::ActiveHigh,
                    PinTrigger::Edge,
                    true,
                    0,
                ),
                Err(RedirectionEntryError::VectorOutOfRange)
            );
        }
    }

    #[test]
    fn nmi_delivery_must_not_carry_a_vector() {
        assert_eq!(
            RedirectionEntry::new(
                0x50,
                DeliveryMode::Nmi,
                DestinationMode::Physical,
                PinPolarity::ActiveHigh,
                PinTrigger::Edge,
                true,
                0,
            ),
            Err(RedirectionEntryError::VectorNotAllowedForDeliveryMode)
        );

        assert!(
            RedirectionEntry::new(
                0,
                DeliveryMode::Nmi,
                DestinationMode::Physical,
                PinPolarity::ActiveHigh,
                PinTrigger::Edge,
                true,
                0,
            )
            .is_ok()
        );
    }

    #[test]
    fn redirection_registers_are_a_pair_per_entry_and_bounded() {
        assert_eq!(redirection_registers(0), Ok((0x10, 0x11)));
        assert_eq!(redirection_registers(2), Ok((0x14, 0x15)));
        assert_eq!(redirection_registers(23), Ok((0x3e, 0x3f)));
        assert_eq!(
            redirection_registers(MAX_REDIRECTION_INDEX + 1),
            Err(RedirectionEntryError::IndexOutOfRange)
        );
    }

    #[test]
    fn version_register_reports_the_implemented_entry_count() {
        // QEMU's I/O APIC: version 0x20, 24 redirection entries.
        assert_eq!(redirection_entry_count(0x0017_0020), 24);
    }
}
