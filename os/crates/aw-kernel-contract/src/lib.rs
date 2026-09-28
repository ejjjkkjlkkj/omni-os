#![no_std]
#![forbid(unsafe_code)]

/// CPU architecture intentionally supported by the public boot contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Architecture {
    X86_64,
}

/// Firmware environments accepted by the project.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Firmware {
    Uefi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Usable,
    Firmware,
    Runtime,
    Acpi,
    Mmio,
    Reserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalMemoryRegion {
    pub start: u64,
    pub length: u64,
    pub kind: MemoryRegionKind,
}

impl PhysicalMemoryRegion {
    #[must_use]
    pub const fn end_exclusive(self) -> Option<u64> {
        self.start.checked_add(self.length)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.length == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferInfo {
    pub physical_address: u64,
    pub byte_len: u64,
    pub width: u32,
    pub height: u32,
    pub stride_pixels: u32,
    pub format: PixelFormat,
}

impl FramebufferInfo {
    #[must_use]
    pub const fn dimensions_are_valid(self) -> bool {
        self.width > 0 && self.height > 0 && self.stride_pixels >= self.width && self.byte_len > 0
    }
}

/// Data passed from the boot environment into the x86-64 kernel entry path.
///
/// The contract intentionally owns no allocator-backed objects so it can be
/// consumed before the kernel heap exists.
#[derive(Clone, Copy, Debug)]
pub struct BootInfo<'a> {
    pub architecture: Architecture,
    pub firmware: Firmware,
    pub memory_regions: &'a [PhysicalMemoryRegion],
    pub framebuffer: Option<FramebufferInfo>,
}

impl BootInfo<'_> {
    #[must_use]
    pub fn basic_invariants_hold(&self) -> bool {
        let memory_map_is_valid = self
            .memory_regions
            .iter()
            .all(|region| !region.is_empty() && region.end_exclusive().is_some());

        let framebuffer_is_valid = self
            .framebuffer
            .is_none_or(FramebufferInfo::dimensions_are_valid);

        memory_map_is_valid && framebuffer_is_valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_overflowing_memory_region() {
        let regions = [PhysicalMemoryRegion {
            start: u64::MAX - 3,
            length: 8,
            kind: MemoryRegionKind::Usable,
        }];
        let info = BootInfo {
            architecture: Architecture::X86_64,
            firmware: Firmware::Uefi,
            memory_regions: &regions,
            framebuffer: None,
        };

        assert!(!info.basic_invariants_hold());
    }

    #[test]
    fn accepts_minimal_valid_boot_info() {
        let regions = [PhysicalMemoryRegion {
            start: 0x10_0000,
            length: 0x20_0000,
            kind: MemoryRegionKind::Usable,
        }];
        let info = BootInfo {
            architecture: Architecture::X86_64,
            firmware: Firmware::Uefi,
            memory_regions: &regions,
            framebuffer: Some(FramebufferInfo {
                physical_address: 0x8000_0000,
                byte_len: 1920 * 1080 * 4,
                width: 1920,
                height: 1080,
                stride_pixels: 1920,
                format: PixelFormat::Bgr,
            }),
        };

        assert!(info.basic_invariants_hold());
    }
}
