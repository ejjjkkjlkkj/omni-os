//! Pure encoding for x86 MSI/MSI-X message address and data words.
//!
//! An MSI is a posted memory write the device performs into the local APIC's
//! architectural message window. There is no interrupt pin and no I/O APIC in
//! the path: the vector is carried in the written *data*, and the destination
//! CPU in the written *address*. Getting either field wrong produces a write
//! that silently lands somewhere harmless and an interrupt that never arrives,
//! which is why both words are built and validated here rather than open-coded
//! at each call site.

use crate::ioapic::{DeliveryMode, PinTrigger};

/// Base of the architectural local-APIC message window. Writes here are
/// interpreted as interrupt messages rather than ordinary memory traffic.
pub const MSI_ADDRESS_BASE: u32 = 0xfee0_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MsiError {
    /// The vector is inside the CPU exception range, or is reserved.
    VectorOutOfRange,
    /// A delivery mode that ignores the vector field was given a non-zero one.
    VectorNotAllowedForDeliveryMode,
}

/// The address/data pair written into a device's MSI capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsiMessage {
    pub address: u32,
    pub data: u32,
}

impl MsiMessage {
    const REDIRECTION_HINT: u32 = 1 << 3;
    const LOGICAL_DESTINATION: u32 = 1 << 2;
    const LEVEL_ASSERT: u32 = 1 << 14;
    const LEVEL_TRIGGERED: u32 = 1 << 15;

    /// Build a message targeting one CPU by APIC ID.
    ///
    /// Physical destination with no redirection hint is the only form that
    /// needs no logical-destination setup in the local APICs, which makes it
    /// the correct choice for bring-up.
    pub const fn physical(
        destination_apic_id: u8,
        vector: u8,
        delivery_mode: DeliveryMode,
        trigger: PinTrigger,
    ) -> Result<Self, MsiError> {
        if delivery_mode.uses_vector() {
            if vector < crate::FIRST_USABLE_INTERRUPT_VECTOR || vector == 0xff {
                return Err(MsiError::VectorOutOfRange);
            }
        } else if vector != 0 {
            return Err(MsiError::VectorNotAllowedForDeliveryMode);
        }

        let trigger_bits = match trigger {
            PinTrigger::Edge => 0,
            PinTrigger::Level => Self::LEVEL_TRIGGERED | Self::LEVEL_ASSERT,
        };

        Ok(Self {
            address: MSI_ADDRESS_BASE | ((destination_apic_id as u32) << 12),
            data: vector as u32 | ((delivery_mode.bits() as u32) << 8) | trigger_bits,
        })
    }

    /// Whether this message addresses a logical destination rather than an
    /// APIC ID. Always false for messages built by [`Self::physical`]; useful
    /// when decoding what firmware or another driver left in a capability.
    #[must_use]
    pub const fn is_logical_destination(self) -> bool {
        self.address & Self::LOGICAL_DESTINATION != 0
    }

    #[must_use]
    pub const fn uses_redirection_hint(self) -> bool {
        self.address & Self::REDIRECTION_HINT != 0
    }

    #[must_use]
    pub const fn destination_apic_id(self) -> u8 {
        ((self.address >> 12) & 0xff) as u8
    }

    #[must_use]
    pub const fn vector(self) -> u8 {
        self.data as u8
    }

    /// Upper 32 bits of the 64-bit message address. The local APIC window is
    /// below 4 GiB on every x86-64 platform, so this is always zero; a 64-bit
    /// capable capability still has to be given the word.
    #[must_use]
    pub const fn address_high(self) -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_fixed_edge_message_to_the_bootstrap_processor() {
        let message = MsiMessage::physical(0, 0x51, DeliveryMode::Fixed, PinTrigger::Edge)
            .expect("device vector is valid");

        assert_eq!(message.address, 0xfee0_0000);
        assert_eq!(message.data, 0x0000_0051);
        assert_eq!(message.destination_apic_id(), 0);
        assert_eq!(message.vector(), 0x51);
        assert!(!message.is_logical_destination());
        assert!(!message.uses_redirection_hint());
        assert_eq!(message.address_high(), 0);
    }

    #[test]
    fn destination_apic_id_lands_in_the_address_not_the_data() {
        let message = MsiMessage::physical(3, 0x60, DeliveryMode::Fixed, PinTrigger::Edge)
            .expect("valid message");

        assert_eq!(message.address, 0xfee0_3000);
        assert_eq!(message.data, 0x0000_0060);
    }

    #[test]
    fn level_triggered_messages_set_both_level_and_assert() {
        let message = MsiMessage::physical(0, 0x52, DeliveryMode::Fixed, PinTrigger::Level)
            .expect("valid message");

        assert_eq!(message.data, 0x0000_c052);
    }

    #[test]
    fn rejects_vectors_reserved_for_cpu_exceptions() {
        assert_eq!(
            MsiMessage::physical(0, 14, DeliveryMode::Fixed, PinTrigger::Edge),
            Err(MsiError::VectorOutOfRange)
        );
        assert_eq!(
            MsiMessage::physical(0, 0xff, DeliveryMode::Fixed, PinTrigger::Edge),
            Err(MsiError::VectorOutOfRange)
        );
    }

    #[test]
    fn nmi_delivery_must_not_carry_a_vector() {
        assert_eq!(
            MsiMessage::physical(0, 0x50, DeliveryMode::Nmi, PinTrigger::Edge),
            Err(MsiError::VectorNotAllowedForDeliveryMode)
        );
        assert!(MsiMessage::physical(0, 0, DeliveryMode::Nmi, PinTrigger::Edge).is_ok());
    }
}
