#![no_std]
#![forbid(unsafe_code)]

pub mod image;

pub use image::{
    AWKN_IMAGE_HEADER_BYTES, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, AwknImageHeader,
    ImageHeaderError,
};

pub const KERNEL_HANDOFF_MAGIC: u64 = 0x4157_4b48_4f46_4631;
pub const KERNEL_HANDOFF_ABI_VERSION: u32 = 4;
pub const HANDOFF_FLAG_FRAMEBUFFER_PRESENT: u64 = 1 << 0;
pub const HANDOFF_FLAG_PCIE_ECAM_PRESENT: u64 = 1 << 1;
pub const HANDOFF_FLAG_MEMORY_MAP_PRESENT: u64 = 1 << 2;
pub const MAX_PCIE_ECAM_REGIONS: usize = 4;
pub const UEFI_PAGE_SIZE: u64 = 4096;
pub const UEFI_MEMORY_TYPE_CONVENTIONAL: u32 = 7;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoffPixelFormat {
    Unknown = 0,
    Rgb = 1,
    Bgr = 2,
    Bitmask = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferHandoff {
    pub physical_address: u64,
    pub byte_len: u64,
    pub width: u32,
    pub height: u32,
    pub stride_pixels: u32,
    pub pixel_format: HandoffPixelFormat,
}

impl FramebufferHandoff {
    pub const NONE: Self = Self {
        physical_address: 0,
        byte_len: 0,
        width: 0,
        height: 0,
        stride_pixels: 0,
        pixel_format: HandoffPixelFormat::Unknown,
    };

    #[must_use]
    pub const fn dimensions_are_valid(self) -> bool {
        self.physical_address != 0
            && self.byte_len != 0
            && self.width != 0
            && self.height != 0
            && self.stride_pixels >= self.width
    }
}

/// Physical allocation containing the position-independent native kernel.
///
/// `image_byte_len` is the exact flat image length. `allocation_byte_len` is
/// the page-rounded UEFI allocation that must remain owned by the kernel and
/// mapped before any future CR3 switch.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelImageHandoff {
    pub physical_address: u64,
    pub image_byte_len: u64,
    pub allocation_byte_len: u64,
}

impl KernelImageHandoff {
    pub const NONE: Self = Self {
        physical_address: 0,
        image_byte_len: 0,
        allocation_byte_len: 0,
    };

    #[must_use]
    pub fn allocation_end_exclusive(self) -> Option<u64> {
        self.physical_address.checked_add(self.allocation_byte_len)
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.physical_address != 0
            && self.physical_address.is_multiple_of(UEFI_PAGE_SIZE)
            && self.image_byte_len != 0
            && self.allocation_byte_len != 0
            && self.allocation_byte_len.is_multiple_of(UEFI_PAGE_SIZE)
            && self.image_byte_len <= self.allocation_byte_len
            && self.allocation_end_exclusive().is_some()
    }
}

/// Architecture-neutral memory descriptor used by the kernel handoff.
///
/// This intentionally does not expose the layout of `uefi-rs` or firmware
/// descriptors. The loader normalizes the final UEFI memory map into this
/// stable representation before transferring control to the native kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryDescriptorHandoff {
    pub memory_type: u32,
    pub reserved: u32,
    pub physical_start: u64,
    pub page_count: u64,
    pub attributes: u64,
}

impl MemoryDescriptorHandoff {
    pub const NONE: Self = Self {
        memory_type: 0,
        reserved: 0,
        physical_start: 0,
        page_count: 0,
        attributes: 0,
    };

    #[must_use]
    pub fn byte_len(self) -> Option<u64> {
        self.page_count.checked_mul(UEFI_PAGE_SIZE)
    }

    #[must_use]
    pub fn physical_end_exclusive(self) -> Option<u64> {
        self.physical_start.checked_add(self.byte_len()?)
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.reserved == 0
            && self.page_count != 0
            && self.physical_start.is_multiple_of(UEFI_PAGE_SIZE)
            && self.physical_end_exclusive().is_some()
    }
}

/// Location and shape of a normalized memory-descriptor array.
///
/// `buffer_address` is the linear address valid at kernel entry. The kernel
/// must copy or map this buffer before replacing the firmware page tables.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryMapHandoff {
    pub buffer_address: u64,
    pub byte_len: u64,
    pub entry_count: u32,
    pub descriptor_size: u32,
}

impl MemoryMapHandoff {
    pub const NONE: Self = Self {
        buffer_address: 0,
        byte_len: 0,
        entry_count: 0,
        descriptor_size: 0,
    };

    #[must_use]
    pub fn is_valid(self) -> bool {
        let descriptor_size = core::mem::size_of::<MemoryDescriptorHandoff>() as u32;
        if self.buffer_address == 0
            || !self
                .buffer_address
                .is_multiple_of(core::mem::align_of::<MemoryDescriptorHandoff>() as u64)
            || self.entry_count == 0
            || self.descriptor_size != descriptor_size
        {
            return false;
        }

        u64::from(self.entry_count).checked_mul(u64::from(self.descriptor_size))
            == Some(self.byte_len)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PciEcamHandoff {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
    pub reserved: u32,
}

impl PciEcamHandoff {
    pub const NONE: Self = Self {
        base_address: 0,
        segment_group: 0,
        start_bus: 0,
        end_bus: 0,
        reserved: 0,
    };

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.base_address != 0
            && self.base_address & 0x000f_ffff == 0
            && self.start_bus <= self.end_bus
            && self.reserved == 0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelHandoff {
    pub magic: u64,
    pub abi_version: u32,
    pub struct_size: u32,
    pub flags: u64,
    pub acpi_rsdp: u64,
    pub kernel_image: KernelImageHandoff,
    pub memory_map: MemoryMapHandoff,
    pub framebuffer: FramebufferHandoff,
    pub pcie_ecam_count: u32,
    pub reserved: u32,
    pub pcie_ecam: [PciEcamHandoff; MAX_PCIE_ECAM_REGIONS],
}

impl KernelHandoff {
    #[must_use]
    pub const fn new(
        acpi_rsdp: u64,
        kernel_image: KernelImageHandoff,
        memory_map: MemoryMapHandoff,
        framebuffer: Option<FramebufferHandoff>,
        pcie_ecam: [PciEcamHandoff; MAX_PCIE_ECAM_REGIONS],
        pcie_ecam_count: u32,
    ) -> Self {
        let framebuffer_present = framebuffer.is_some();
        let ecam_present = pcie_ecam_count != 0;
        let memory_map_present = memory_map.entry_count != 0;
        let mut flags = 0;
        if framebuffer_present {
            flags |= HANDOFF_FLAG_FRAMEBUFFER_PRESENT;
        }
        if ecam_present {
            flags |= HANDOFF_FLAG_PCIE_ECAM_PRESENT;
        }
        if memory_map_present {
            flags |= HANDOFF_FLAG_MEMORY_MAP_PRESENT;
        }

        Self {
            magic: KERNEL_HANDOFF_MAGIC,
            abi_version: KERNEL_HANDOFF_ABI_VERSION,
            struct_size: core::mem::size_of::<Self>() as u32,
            flags,
            acpi_rsdp,
            kernel_image,
            memory_map,
            framebuffer: match framebuffer {
                Some(framebuffer) => framebuffer,
                None => FramebufferHandoff::NONE,
            },
            pcie_ecam_count,
            reserved: 0,
            pcie_ecam,
        }
    }

    pub fn validate(&self) -> Result<(), HandoffError> {
        if self.magic != KERNEL_HANDOFF_MAGIC {
            return Err(HandoffError::InvalidMagic);
        }
        if self.abi_version != KERNEL_HANDOFF_ABI_VERSION {
            return Err(HandoffError::UnsupportedAbiVersion);
        }
        if self.struct_size < core::mem::size_of::<Self>() as u32 {
            return Err(HandoffError::InvalidStructSize);
        }
        if self.reserved != 0 {
            return Err(HandoffError::InvalidReservedField);
        }
        if self.acpi_rsdp == 0 {
            return Err(HandoffError::MissingAcpiRsdp);
        }
        if !self.kernel_image.is_valid() {
            return Err(HandoffError::InvalidKernelImage);
        }

        let memory_map_present = self.flags & HANDOFF_FLAG_MEMORY_MAP_PRESENT != 0;
        if !memory_map_present {
            return Err(HandoffError::EmptyMemoryMap);
        }
        if !self.memory_map.is_valid() {
            return Err(HandoffError::InvalidMemoryMap);
        }

        let framebuffer_present = self.flags & HANDOFF_FLAG_FRAMEBUFFER_PRESENT != 0;
        if framebuffer_present && !self.framebuffer.dimensions_are_valid() {
            return Err(HandoffError::InvalidFramebuffer);
        }
        if !framebuffer_present && self.framebuffer != FramebufferHandoff::NONE {
            return Err(HandoffError::UnexpectedFramebuffer);
        }

        let ecam_present = self.flags & HANDOFF_FLAG_PCIE_ECAM_PRESENT != 0;
        let ecam_count = self.pcie_ecam_count as usize;
        if ecam_count > MAX_PCIE_ECAM_REGIONS {
            return Err(HandoffError::TooManyPcieEcamRegions);
        }
        if ecam_present != (ecam_count != 0) {
            return Err(HandoffError::InconsistentPcieEcamFlag);
        }

        let mut index = 0;
        while index < MAX_PCIE_ECAM_REGIONS {
            let region = self.pcie_ecam[index];
            if index < ecam_count {
                if !region.is_valid() {
                    return Err(HandoffError::InvalidPcieEcamRegion);
                }
            } else if region != PciEcamHandoff::NONE {
                return Err(HandoffError::UnexpectedPcieEcamRegion);
            }
            index += 1;
        }

        Ok(())
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoffError {
    InvalidMagic = 1,
    UnsupportedAbiVersion = 2,
    InvalidStructSize = 3,
    MissingAcpiRsdp = 4,
    EmptyMemoryMap = 5,
    InvalidFramebuffer = 6,
    UnexpectedFramebuffer = 7,
    InvalidReservedField = 8,
    TooManyPcieEcamRegions = 9,
    InconsistentPcieEcamFlag = 10,
    InvalidPcieEcamRegion = 11,
    UnexpectedPcieEcamRegion = 12,
    InvalidMemoryMap = 13,
    InvalidKernelImage = 14,
}

pub fn enter(handoff: &KernelHandoff) -> Result<(), HandoffError> {
    handoff.validate()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_FRAMEBUFFER: FramebufferHandoff = FramebufferHandoff {
        physical_address: 0x8000_0000,
        byte_len: 1280 * 800 * 4,
        width: 1280,
        height: 800,
        stride_pixels: 1280,
        pixel_format: HandoffPixelFormat::Bgr,
    };

    const VALID_KERNEL_IMAGE: KernelImageHandoff = KernelImageHandoff {
        physical_address: 0x40_0000,
        image_byte_len: 7000,
        allocation_byte_len: 8192,
    };

    const VALID_ECAM: PciEcamHandoff = PciEcamHandoff {
        base_address: 0xe000_0000,
        segment_group: 0,
        start_bus: 0,
        end_bus: 0xff,
        reserved: 0,
    };

    const VALID_MEMORY_DESCRIPTOR: MemoryDescriptorHandoff = MemoryDescriptorHandoff {
        memory_type: UEFI_MEMORY_TYPE_CONVENTIONAL,
        reserved: 0,
        physical_start: 0x10_0000,
        page_count: 256,
        attributes: 0,
    };

    const fn empty_ecam() -> [PciEcamHandoff; MAX_PCIE_ECAM_REGIONS] {
        [PciEcamHandoff::NONE; MAX_PCIE_ECAM_REGIONS]
    }

    fn valid_memory_map() -> MemoryMapHandoff {
        let descriptor_size = core::mem::size_of::<MemoryDescriptorHandoff>() as u32;
        MemoryMapHandoff {
            buffer_address: 0x20_0000,
            byte_len: u64::from(descriptor_size) * 127,
            entry_count: 127,
            descriptor_size,
        }
    }

    fn valid_handoff() -> KernelHandoff {
        let mut ecam = empty_ecam();
        ecam[0] = VALID_ECAM;
        KernelHandoff::new(
            0xf000_0000,
            VALID_KERNEL_IMAGE,
            valid_memory_map(),
            Some(VALID_FRAMEBUFFER),
            ecam,
            1,
        )
    }

    #[test]
    fn accepts_valid_handoff() {
        assert_eq!(enter(&valid_handoff()), Ok(()));
    }

    #[test]
    fn accepts_handoff_without_optional_devices() {
        let handoff = KernelHandoff::new(
            0xf000_0000,
            VALID_KERNEL_IMAGE,
            valid_memory_map(),
            None,
            empty_ecam(),
            0,
        );
        assert_eq!(enter(&handoff), Ok(()));
    }

    #[test]
    fn validates_kernel_image_allocation() {
        assert!(VALID_KERNEL_IMAGE.is_valid());
        assert_eq!(
            VALID_KERNEL_IMAGE.allocation_end_exclusive(),
            Some(0x40_2000)
        );
    }

    #[test]
    fn rejects_invalid_kernel_image_allocation() {
        let mut image = VALID_KERNEL_IMAGE;
        image.physical_address += 1;
        assert!(!image.is_valid());

        image = VALID_KERNEL_IMAGE;
        image.image_byte_len = image.allocation_byte_len + 1;
        assert!(!image.is_valid());

        image = VALID_KERNEL_IMAGE;
        image.allocation_byte_len = 7000;
        assert!(!image.is_valid());

        image = VALID_KERNEL_IMAGE;
        image.physical_address = !(UEFI_PAGE_SIZE - 1);
        image.allocation_byte_len = 8192;
        assert!(!image.is_valid());
    }

    #[test]
    fn validates_normalized_memory_descriptor() {
        assert!(VALID_MEMORY_DESCRIPTOR.is_valid());
        assert_eq!(
            VALID_MEMORY_DESCRIPTOR.byte_len(),
            Some(256 * UEFI_PAGE_SIZE)
        );
        assert_eq!(
            VALID_MEMORY_DESCRIPTOR.physical_end_exclusive(),
            Some(0x10_0000 + 256 * UEFI_PAGE_SIZE)
        );
    }

    #[test]
    fn rejects_invalid_normalized_memory_descriptors() {
        let mut descriptor = VALID_MEMORY_DESCRIPTOR;
        descriptor.physical_start += 1;
        assert!(!descriptor.is_valid());

        descriptor = VALID_MEMORY_DESCRIPTOR;
        descriptor.page_count = 0;
        assert!(!descriptor.is_valid());

        descriptor = VALID_MEMORY_DESCRIPTOR;
        descriptor.reserved = 1;
        assert!(!descriptor.is_valid());

        descriptor = VALID_MEMORY_DESCRIPTOR;
        descriptor.page_count = u64::MAX;
        assert!(!descriptor.is_valid());
    }

    #[test]
    fn validates_memory_map_shape() {
        assert!(valid_memory_map().is_valid());
    }

    #[test]
    fn rejects_invalid_memory_map_shape() {
        let descriptor_size = core::mem::size_of::<MemoryDescriptorHandoff>() as u32;
        let valid = MemoryMapHandoff {
            buffer_address: 0x20_0000,
            byte_len: u64::from(descriptor_size) * 4,
            entry_count: 4,
            descriptor_size,
        };

        let mut map = valid;
        map.buffer_address += 1;
        assert!(!map.is_valid());

        map = valid;
        map.entry_count = 0;
        assert!(!map.is_valid());

        map = valid;
        map.descriptor_size = descriptor_size + 8;
        assert!(!map.is_valid());

        map = valid;
        map.byte_len -= 1;
        assert!(!map.is_valid());
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut handoff = valid_handoff();
        handoff.magic ^= 1;
        assert_eq!(enter(&handoff), Err(HandoffError::InvalidMagic));
    }

    #[test]
    fn rejects_empty_memory_map() {
        let handoff = KernelHandoff::new(
            0xf000_0000,
            VALID_KERNEL_IMAGE,
            MemoryMapHandoff::NONE,
            None,
            empty_ecam(),
            0,
        );
        assert_eq!(enter(&handoff), Err(HandoffError::EmptyMemoryMap));
    }

    #[test]
    fn rejects_invalid_memory_map() {
        let mut memory_map = valid_memory_map();
        memory_map.byte_len -= 1;
        let handoff = KernelHandoff::new(
            0xf000_0000,
            VALID_KERNEL_IMAGE,
            memory_map,
            None,
            empty_ecam(),
            0,
        );
        assert_eq!(enter(&handoff), Err(HandoffError::InvalidMemoryMap));
    }

    #[test]
    fn rejects_invalid_kernel_image() {
        let mut image = VALID_KERNEL_IMAGE;
        image.image_byte_len = 0;
        let handoff = KernelHandoff::new(
            0xf000_0000,
            image,
            valid_memory_map(),
            None,
            empty_ecam(),
            0,
        );
        assert_eq!(enter(&handoff), Err(HandoffError::InvalidKernelImage));
    }

    #[test]
    fn rejects_invalid_framebuffer() {
        let mut framebuffer = VALID_FRAMEBUFFER;
        framebuffer.stride_pixels = framebuffer.width - 1;
        let handoff = KernelHandoff::new(
            0xf000_0000,
            VALID_KERNEL_IMAGE,
            valid_memory_map(),
            Some(framebuffer),
            empty_ecam(),
            0,
        );
        assert_eq!(enter(&handoff), Err(HandoffError::InvalidFramebuffer));
    }

    #[test]
    fn rejects_inconsistent_ecam_flag() {
        let mut handoff = valid_handoff();
        handoff.flags &= !HANDOFF_FLAG_PCIE_ECAM_PRESENT;
        assert_eq!(enter(&handoff), Err(HandoffError::InconsistentPcieEcamFlag));
    }

    #[test]
    fn rejects_nonzero_unused_ecam_slot() {
        let mut handoff = valid_handoff();
        handoff.pcie_ecam[1] = VALID_ECAM;
        assert_eq!(enter(&handoff), Err(HandoffError::UnexpectedPcieEcamRegion));
    }
}
