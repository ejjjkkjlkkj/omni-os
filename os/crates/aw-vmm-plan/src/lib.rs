#![no_std]
#![forbid(unsafe_code)]

use aw_kernel_core::{
    HANDOFF_FLAG_FRAMEBUFFER_PRESENT, HANDOFF_FLAG_PCIE_ECAM_PRESENT, KernelHandoff,
};
use aw_memory::PhysicalRange;

pub const PCIE_ECAM_BYTES_PER_BUS: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalPreservationKind {
    KernelImage,
    AcpiRsdp,
    Framebuffer,
    PcieEcam { index: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalPreservation {
    kind: PhysicalPreservationKind,
    range: PhysicalRange,
}

impl PhysicalPreservation {
    #[must_use]
    pub const fn kind(self) -> PhysicalPreservationKind {
        self.kind
    }

    #[must_use]
    pub const fn range(self) -> PhysicalRange {
        self.range
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryMapCopyRequirement {
    linear_address: u64,
    byte_len: u64,
}

impl MemoryMapCopyRequirement {
    #[must_use]
    pub const fn linear_address(self) -> u64 {
        self.linear_address
    }

    #[must_use]
    pub const fn byte_len(self) -> u64 {
        self.byte_len
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreservationPlanError {
    InvalidHandoff,
    InvalidPhysicalRange,
    Overlap,
    Capacity,
    ArithmeticOverflow,
}

/// Compact list of physical mappings that must survive the first page-table switch.
///
/// The normalized UEFI memory-map buffer is deliberately represented as a copy requirement rather
/// than a physical mapping. Its handoff contract only guarantees a linear address at kernel entry,
/// so treating that address as physical would be an unsafe assumption.
pub struct PreservationPlan<const RANGES: usize> {
    ranges: [Option<PhysicalPreservation>; RANGES],
    len: usize,
    memory_map_copy: MemoryMapCopyRequirement,
}

impl<const RANGES: usize> PreservationPlan<RANGES> {
    pub fn from_handoff(handoff: &KernelHandoff) -> Result<Self, PreservationPlanError> {
        if handoff.validate().is_err() {
            return Err(PreservationPlanError::InvalidHandoff);
        }

        let mut plan = Self {
            ranges: [None; RANGES],
            len: 0,
            memory_map_copy: MemoryMapCopyRequirement {
                linear_address: handoff.memory_map.buffer_address,
                byte_len: handoff.memory_map.byte_len,
            },
        };

        plan.push(
            PhysicalPreservationKind::KernelImage,
            PhysicalRange::covering_byte_range(
                handoff.kernel_image.physical_address,
                handoff.kernel_image.allocation_byte_len,
            )
            .ok_or(PreservationPlanError::InvalidPhysicalRange)?,
        )?;

        plan.push(
            PhysicalPreservationKind::AcpiRsdp,
            PhysicalRange::covering_byte_range(handoff.acpi_rsdp, 1)
                .ok_or(PreservationPlanError::InvalidPhysicalRange)?,
        )?;

        if handoff.flags & HANDOFF_FLAG_FRAMEBUFFER_PRESENT != 0 {
            plan.push(
                PhysicalPreservationKind::Framebuffer,
                PhysicalRange::covering_byte_range(
                    handoff.framebuffer.physical_address,
                    handoff.framebuffer.byte_len,
                )
                .ok_or(PreservationPlanError::InvalidPhysicalRange)?,
            )?;
        }

        if handoff.flags & HANDOFF_FLAG_PCIE_ECAM_PRESENT != 0 {
            for (index, region) in handoff
                .pcie_ecam
                .iter()
                .copied()
                .enumerate()
                .take(handoff.pcie_ecam_count as usize)
            {
                let bus_count = u64::from(region.end_bus) - u64::from(region.start_bus) + 1;
                let byte_len = bus_count
                    .checked_mul(PCIE_ECAM_BYTES_PER_BUS)
                    .ok_or(PreservationPlanError::ArithmeticOverflow)?;
                plan.push(
                    PhysicalPreservationKind::PcieEcam { index: index as u8 },
                    PhysicalRange::covering_byte_range(region.base_address, byte_len)
                        .ok_or(PreservationPlanError::InvalidPhysicalRange)?,
                )?;
            }
        }

        Ok(plan)
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<PhysicalPreservation> {
        if index >= self.len {
            return None;
        }
        self.ranges[index]
    }

    #[must_use]
    pub const fn memory_map_copy(&self) -> MemoryMapCopyRequirement {
        self.memory_map_copy
    }

    fn push(
        &mut self,
        kind: PhysicalPreservationKind,
        range: PhysicalRange,
    ) -> Result<(), PreservationPlanError> {
        for existing in self.ranges[..self.len].iter().flatten() {
            if existing.range().overlaps(range) {
                return Err(PreservationPlanError::Overlap);
            }
        }

        if self.len >= RANGES {
            return Err(PreservationPlanError::Capacity);
        }

        self.ranges[self.len] = Some(PhysicalPreservation { kind, range });
        self.len += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::*;
    use aw_kernel_core::{
        FramebufferHandoff, HandoffPixelFormat, KERNEL_HANDOFF_MAGIC, KernelImageHandoff,
        MAX_PCIE_ECAM_REGIONS, MemoryDescriptorHandoff, MemoryMapHandoff, PciEcamHandoff,
    };

    fn valid_handoff() -> KernelHandoff {
        let mut ecam = [PciEcamHandoff::NONE; MAX_PCIE_ECAM_REGIONS];
        ecam[0] = PciEcamHandoff {
            base_address: 0xe000_0000,
            segment_group: 0,
            start_bus: 0,
            end_bus: 1,
            reserved: 0,
        };

        KernelHandoff::new(
            0x7f00_0123,
            KernelImageHandoff {
                physical_address: 0x40_0000,
                image_byte_len: 7000,
                allocation_byte_len: 8192,
            },
            MemoryMapHandoff {
                buffer_address: 0x1000_0000,
                byte_len: size_of::<MemoryDescriptorHandoff>() as u64,
                entry_count: 1,
                descriptor_size: size_of::<MemoryDescriptorHandoff>() as u32,
            },
            Some(FramebufferHandoff {
                physical_address: 0xe100_0123,
                byte_len: 0x1fff,
                width: 800,
                height: 600,
                stride_pixels: 800,
                pixel_format: HandoffPixelFormat::Bgr,
            }),
            ecam,
            1,
        )
    }

    #[test]
    fn builds_compact_preservation_plan_from_valid_handoff() {
        let handoff = valid_handoff();
        let plan = PreservationPlan::<4>::from_handoff(&handoff).unwrap();

        assert_eq!(plan.len(), 4);
        assert!(!plan.is_empty());
        assert_eq!(
            plan.get(0).unwrap(),
            PhysicalPreservation {
                kind: PhysicalPreservationKind::KernelImage,
                range: PhysicalRange::new(0x40_0000, 0x40_2000).unwrap(),
            }
        );
        assert_eq!(
            plan.get(1).unwrap(),
            PhysicalPreservation {
                kind: PhysicalPreservationKind::AcpiRsdp,
                range: PhysicalRange::new(0x7f00_0000, 0x7f00_1000).unwrap(),
            }
        );
        assert_eq!(
            plan.get(2).unwrap(),
            PhysicalPreservation {
                kind: PhysicalPreservationKind::Framebuffer,
                range: PhysicalRange::new(0xe100_0000, 0xe100_3000).unwrap(),
            }
        );
        assert_eq!(
            plan.get(3).unwrap(),
            PhysicalPreservation {
                kind: PhysicalPreservationKind::PcieEcam { index: 0 },
                range: PhysicalRange::new(0xe000_0000, 0xe020_0000).unwrap(),
            }
        );
        assert_eq!(plan.get(4), None);

        let copy = plan.memory_map_copy();
        assert_eq!(copy.linear_address(), 0x1000_0000);
        assert_eq!(copy.byte_len(), size_of::<MemoryDescriptorHandoff>() as u64);
    }

    #[test]
    fn rejects_overlapping_required_physical_ranges() {
        let mut handoff = valid_handoff();
        handoff.framebuffer.physical_address = 0x40_0800;
        handoff.framebuffer.byte_len = 0x100;

        assert!(matches!(
            PreservationPlan::<4>::from_handoff(&handoff),
            Err(PreservationPlanError::Overlap)
        ));
    }

    #[test]
    fn rejects_insufficient_fixed_capacity() {
        let handoff = valid_handoff();
        assert!(matches!(
            PreservationPlan::<3>::from_handoff(&handoff),
            Err(PreservationPlanError::Capacity)
        ));
    }

    #[test]
    fn rejects_invalid_handoff_before_planning() {
        let mut handoff = valid_handoff();
        handoff.magic = KERNEL_HANDOFF_MAGIC ^ 1;

        assert!(matches!(
            PreservationPlan::<4>::from_handoff(&handoff),
            Err(PreservationPlanError::InvalidHandoff)
        ));
    }
}
