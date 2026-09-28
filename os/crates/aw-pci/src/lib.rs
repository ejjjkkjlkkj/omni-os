#![no_std]
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PciAddress {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    pub const fn new(segment: u16, bus: u8, device: u8, function: u8) -> Option<Self> {
        if device > 31 || function > 7 {
            return None;
        }
        Some(Self {
            segment,
            bus,
            device,
            function,
        })
    }

    pub const fn ecam_offset(self, register_offset: u16) -> Option<u64> {
        if register_offset > 0x0fff {
            return None;
        }
        Some(
            ((self.bus as u64) << 20)
                | ((self.device as u64) << 15)
                | ((self.function as u64) << 12)
                | register_offset as u64,
        )
    }

    pub const fn mechanism1_address(self, register_offset: u8) -> Option<u32> {
        if self.segment != 0 || register_offset > 0xfc || register_offset & 0x03 != 0 {
            return None;
        }

        Some(
            0x8000_0000
                | ((self.bus as u32) << 16)
                | ((self.device as u32) << 11)
                | ((self.function as u32) << 8)
                | register_offset as u32,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PciClassCode {
    pub base: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

impl PciClassCode {
    pub const fn new(base: u8, subclass: u8, programming_interface: u8) -> Self {
        Self {
            base,
            subclass,
            programming_interface,
        }
    }

    pub const fn from_class_revision_register(value: u32) -> Self {
        Self {
            base: (value >> 24) as u8,
            subclass: (value >> 16) as u8,
            programming_interface: (value >> 8) as u8,
        }
    }

    pub const fn base_class(self) -> PciBaseClass {
        match self.base {
            0x01 => PciBaseClass::MassStorage,
            0x02 => PciBaseClass::Network,
            0x03 => PciBaseClass::Display,
            0x04 => PciBaseClass::Multimedia,
            0x06 => PciBaseClass::Bridge,
            0x0c => PciBaseClass::SerialBus,
            value => PciBaseClass::Other(value),
        }
    }

    pub const fn is_nvme(self) -> bool {
        self.base == 0x01 && self.subclass == 0x08 && self.programming_interface == 0x02
    }

    pub const fn is_ahci(self) -> bool {
        self.base == 0x01 && self.subclass == 0x06 && self.programming_interface == 0x01
    }

    pub const fn is_xhci(self) -> bool {
        self.base == 0x0c && self.subclass == 0x03 && self.programming_interface == 0x30
    }

    pub const fn is_hda(self) -> bool {
        self.base == 0x04 && self.subclass == 0x03
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciBaseClass {
    MassStorage,
    Network,
    Display,
    Multimedia,
    Bridge,
    SerialBus,
    Other(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PciDeviceIdentity {
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
    pub class: PciClassCode,
}

impl PciDeviceIdentity {
    pub const fn from_config_registers(
        vendor_device: u32,
        class_revision: u32,
        subsystem: Option<u32>,
    ) -> Self {
        let (subsystem_vendor_id, subsystem_device_id) = match subsystem {
            Some(value) => (Some(value as u16), Some((value >> 16) as u16)),
            None => (None, None),
        };

        Self {
            vendor_id: vendor_device as u16,
            device_id: (vendor_device >> 16) as u16,
            subsystem_vendor_id,
            subsystem_device_id,
            class: PciClassCode::from_class_revision_register(class_revision),
        }
    }

    pub const fn is_present(self) -> bool {
        self.vendor_id != 0xffff
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciBar {
    Io {
        base_address: u32,
    },
    Memory32 {
        base_address: u32,
        prefetchable: bool,
    },
    Memory64 {
        base_address: u64,
        prefetchable: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciBarError {
    MissingUpperDword,
    UnsupportedMemoryType,
}

impl PciBar {
    pub const fn decode(low: u32, high: Option<u32>) -> Result<Option<Self>, PciBarError> {
        if low == 0 {
            return Ok(None);
        }

        if low & 1 != 0 {
            return Ok(Some(Self::Io {
                base_address: low & 0xffff_fffc,
            }));
        }

        let prefetchable = low & 0x08 != 0;
        match (low >> 1) & 0x03 {
            0x00 => Ok(Some(Self::Memory32 {
                base_address: low & 0xffff_fff0,
                prefetchable,
            })),
            0x02 => {
                let Some(high) = high else {
                    return Err(PciBarError::MissingUpperDword);
                };
                Ok(Some(Self::Memory64 {
                    base_address: ((high as u64) << 32) | ((low & 0xffff_fff0) as u64),
                    prefetchable,
                }))
            }
            _ => Err(PciBarError::UnsupportedMemoryType),
        }
    }

    pub const fn is_mmio(self) -> bool {
        matches!(self, Self::Memory32 { .. } | Self::Memory64 { .. })
    }

    pub const fn base_address(self) -> u64 {
        match self {
            Self::Io { base_address } | Self::Memory32 { base_address, .. } => base_address as u64,
            Self::Memory64 { base_address, .. } => base_address,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_device_or_function_numbers() {
        assert!(PciAddress::new(0, 0, 31, 7).is_some());
        assert!(PciAddress::new(0, 0, 32, 0).is_none());
        assert!(PciAddress::new(0, 0, 0, 8).is_none());
    }

    #[test]
    fn computes_standard_pcie_ecam_offsets() {
        let address = PciAddress::new(0, 2, 5, 3).unwrap();
        assert_eq!(
            address.ecam_offset(0x120),
            Some((2_u64 << 20) | (5_u64 << 15) | (3_u64 << 12) | 0x120)
        );
        assert_eq!(address.ecam_offset(0x1000), None);
    }

    #[test]
    fn computes_legacy_mechanism1_address_for_segment_zero() {
        let address = PciAddress::new(0, 2, 5, 3).unwrap();
        assert_eq!(
            address.mechanism1_address(0x08),
            Some(0x8000_0000 | (2 << 16) | (5 << 11) | (3 << 8) | 0x08)
        );
        assert_eq!(address.mechanism1_address(0x09), None);
        assert_eq!(
            PciAddress::new(1, 0, 0, 0).unwrap().mechanism1_address(0),
            None
        );
    }

    #[test]
    fn parses_standard_configuration_registers() {
        let identity =
            PciDeviceIdentity::from_config_registers(0x1234_8086, 0x0108_0201, Some(0xabcd_1043));
        assert_eq!(identity.vendor_id, 0x8086);
        assert_eq!(identity.device_id, 0x1234);
        assert_eq!(identity.subsystem_vendor_id, Some(0x1043));
        assert_eq!(identity.subsystem_device_id, Some(0xabcd));
        assert!(identity.class.is_nvme());
    }

    #[test]
    fn classifies_boot_critical_standard_controllers() {
        assert!(PciClassCode::new(0x01, 0x08, 0x02).is_nvme());
        assert!(PciClassCode::new(0x01, 0x06, 0x01).is_ahci());
        assert!(PciClassCode::new(0x0c, 0x03, 0x30).is_xhci());
        assert!(PciClassCode::new(0x04, 0x03, 0x00).is_hda());
    }

    #[test]
    fn decodes_standard_pci_bars() {
        assert_eq!(
            PciBar::decode(0x0000_c001, None),
            Ok(Some(PciBar::Io {
                base_address: 0x0000_c000
            }))
        );
        assert_eq!(
            PciBar::decode(0xfebf_0008, None),
            Ok(Some(PciBar::Memory32 {
                base_address: 0xfebf_0000,
                prefetchable: true,
            }))
        );
        assert_eq!(
            PciBar::decode(0x3456_7004, Some(0x0000_0012)),
            Ok(Some(PciBar::Memory64 {
                base_address: 0x0000_0012_3456_7000,
                prefetchable: false,
            }))
        );
        assert_eq!(PciBar::decode(0, None), Ok(None));
    }

    #[test]
    fn rejects_incomplete_or_unsupported_memory_bars() {
        assert_eq!(
            PciBar::decode(0x0000_1004, None),
            Err(PciBarError::MissingUpperDword)
        );
        assert_eq!(
            PciBar::decode(0x0000_1002, None),
            Err(PciBarError::UnsupportedMemoryType)
        );
    }

    #[test]
    fn keeps_unknown_classes_forward_compatible() {
        assert_eq!(
            PciClassCode::new(0xff, 0, 0).base_class(),
            PciBaseClass::Other(0xff)
        );
    }
}
