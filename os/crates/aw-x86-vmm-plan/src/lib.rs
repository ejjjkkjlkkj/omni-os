#![no_std]
#![forbid(unsafe_code)]

use aw_vmm_plan::{PhysicalPreservationKind, PreservationPlan};
use aw_x86_paging::{PAGE_SIZE, PageTableFlags, PhysicalFrame, VirtualPage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingHardening {
    Final,
    KernelSectionSplitRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSectionKind {
    Text,
    ReadOnlyData,
    WritableData,
}

/// Page-aligned kernel protection boundaries relative to the kernel allocation base.
///
/// Text always starts at offset zero. Read-only data starts at `text_end_offset`, and writable
/// data/BSS starts at `read_only_end_offset`. The final writable extent is the end of the kernel
/// allocation described by the preservation plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSectionLayout {
    text_end_offset: u64,
    read_only_end_offset: u64,
}

impl KernelSectionLayout {
    #[must_use]
    pub const fn new(text_end_offset: u64, read_only_end_offset: u64) -> Option<Self> {
        if text_end_offset == 0
            || text_end_offset & (PAGE_SIZE - 1) != 0
            || read_only_end_offset & (PAGE_SIZE - 1) != 0
            || read_only_end_offset < text_end_offset
        {
            return None;
        }
        Some(Self {
            text_end_offset,
            read_only_end_offset,
        })
    }

    #[must_use]
    pub const fn text_end_offset(self) -> u64 {
        self.text_end_offset
    }

    #[must_use]
    pub const fn read_only_end_offset(self) -> u64 {
        self.read_only_end_offset
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityMapping {
    kind: PhysicalPreservationKind,
    kernel_section: Option<KernelSectionKind>,
    virtual_page: VirtualPage,
    physical_frame: PhysicalFrame,
    page_count: u64,
    flags: PageTableFlags,
    hardening: MappingHardening,
}

impl IdentityMapping {
    #[must_use]
    pub const fn kind(self) -> PhysicalPreservationKind {
        self.kind
    }

    #[must_use]
    pub const fn kernel_section(self) -> Option<KernelSectionKind> {
        self.kernel_section
    }

    #[must_use]
    pub const fn virtual_page(self) -> VirtualPage {
        self.virtual_page
    }

    #[must_use]
    pub const fn physical_frame(self) -> PhysicalFrame {
        self.physical_frame
    }

    #[must_use]
    pub const fn page_count(self) -> u64 {
        self.page_count
    }

    #[must_use]
    pub const fn flags(self) -> PageTableFlags {
        self.flags
    }

    #[must_use]
    pub const fn hardening(self) -> MappingHardening {
        self.hardening
    }

    /// Returns true when a mapping is simultaneously writable and executable.
    ///
    /// On x86-64 execution is permitted when the NX bit is absent. Permanent W+X mappings are
    /// forbidden by the activation gate even if their hardening marker is otherwise final.
    #[must_use]
    pub fn is_writable_executable(self) -> bool {
        self.flags.contains(PageTableFlags::WRITABLE)
            && !self.flags.contains(PageTableFlags::NO_EXECUTE)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum X86MappingPlanError {
    Capacity,
    InvalidPhysicalAddress,
    NonCanonicalIdentityMapping,
    ArithmeticOverflow,
    InvalidKernelSectionLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivationGateError {
    IncompleteHardening { index: usize },
    WritableExecutable { index: usize },
}

/// Proof that a mapping plan passed the security checks required before a future CR3 switch.
///
/// The field is private so safe downstream code cannot manufacture this token. A future CR3
/// activation boundary must accept this guard rather than a raw mapping plan.
pub struct ActivationGuard<'a, const MAPPINGS: usize> {
    plan: &'a X86IdentityMappingPlan<MAPPINGS>,
}

impl<const MAPPINGS: usize> ActivationGuard<'_, MAPPINGS> {
    #[must_use]
    pub const fn mapping_count(&self) -> usize {
        self.plan.len()
    }
}

/// x86-64 identity mappings required before a future first CR3 switch.
///
/// This layer only translates already validated physical preservation ranges into paging policy.
/// It neither constructs active page tables nor changes CR3. Kernel code/data are intentionally
/// marked as requiring a later section split unless a validated `KernelSectionLayout` is supplied.
pub struct X86IdentityMappingPlan<const MAPPINGS: usize> {
    mappings: [Option<IdentityMapping>; MAPPINGS],
    len: usize,
}

impl<const MAPPINGS: usize> X86IdentityMappingPlan<MAPPINGS> {
    pub fn from_preservation<const RANGES: usize>(
        preservation: &PreservationPlan<RANGES>,
        physical_address_bits: u8,
    ) -> Result<Self, X86MappingPlanError> {
        let mut result = Self::empty();

        let mut index = 0;
        while index < preservation.len() {
            let item = preservation
                .get(index)
                .ok_or(X86MappingPlanError::Capacity)?;
            let range = item.range();
            let (flags, hardening) = Self::preservation_policy(item.kind());
            let mapping = Self::mapping_for_range(
                item.kind(),
                None,
                range.start_address(),
                range.end_address_exclusive(),
                flags,
                hardening,
                physical_address_bits,
            )?;
            result.push(mapping)?;
            index += 1;
        }

        Ok(result)
    }

    /// Builds an activation-ready plan when the kernel allocation has page-aligned W^X sections.
    ///
    /// The kernel allocation is replaced by up to three mappings: text (RX), read-only data
    /// (R+NX), and writable data/BSS (RW+NX). All other preservation mappings retain their
    /// conservative device/firmware policies.
    pub fn from_preservation_with_kernel_layout<const RANGES: usize>(
        preservation: &PreservationPlan<RANGES>,
        physical_address_bits: u8,
        kernel_layout: KernelSectionLayout,
    ) -> Result<Self, X86MappingPlanError> {
        let mut result = Self::empty();

        let mut index = 0;
        while index < preservation.len() {
            let item = preservation
                .get(index)
                .ok_or(X86MappingPlanError::Capacity)?;
            let range = item.range();
            if item.kind() == PhysicalPreservationKind::KernelImage {
                result.push_kernel_sections(
                    range.start_address(),
                    range.end_address_exclusive(),
                    physical_address_bits,
                    kernel_layout,
                )?;
            } else {
                let (flags, hardening) = Self::preservation_policy(item.kind());
                let mapping = Self::mapping_for_range(
                    item.kind(),
                    None,
                    range.start_address(),
                    range.end_address_exclusive(),
                    flags,
                    hardening,
                    physical_address_bits,
                )?;
                result.push(mapping)?;
            }
            index += 1;
        }

        Ok(result)
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
    pub fn get(&self, index: usize) -> Option<IdentityMapping> {
        if index >= self.len {
            return None;
        }
        self.mappings[index]
    }

    /// Validates whether this plan is permitted to reach a future CR3 activation boundary.
    ///
    /// This is intentionally fail-closed. Every mapping must be marked fully hardened and no
    /// mapping may be both writable and executable. The returned guard cannot be created directly
    /// by safe downstream code.
    pub fn activation_guard(&self) -> Result<ActivationGuard<'_, MAPPINGS>, ActivationGateError> {
        let mut index = 0;
        while index < self.len {
            let Some(mapping) = self.mappings[index] else {
                return Err(ActivationGateError::IncompleteHardening { index });
            };

            if mapping.hardening() != MappingHardening::Final {
                return Err(ActivationGateError::IncompleteHardening { index });
            }
            if mapping.is_writable_executable() {
                return Err(ActivationGateError::WritableExecutable { index });
            }
            index += 1;
        }

        Ok(ActivationGuard { plan: self })
    }

    const fn empty() -> Self {
        Self {
            mappings: [None; MAPPINGS],
            len: 0,
        }
    }

    fn preservation_policy(kind: PhysicalPreservationKind) -> (PageTableFlags, MappingHardening) {
        match kind {
            PhysicalPreservationKind::KernelImage => (
                PageTableFlags::WRITABLE,
                MappingHardening::KernelSectionSplitRequired,
            ),
            PhysicalPreservationKind::AcpiRsdp => {
                (PageTableFlags::NO_EXECUTE, MappingHardening::Final)
            }
            PhysicalPreservationKind::Framebuffer | PhysicalPreservationKind::PcieEcam { .. } => (
                PageTableFlags::WRITABLE
                    .union(PageTableFlags::NO_EXECUTE)
                    .union(PageTableFlags::CACHE_DISABLE),
                MappingHardening::Final,
            ),
        }
    }

    fn mapping_for_range(
        kind: PhysicalPreservationKind,
        kernel_section: Option<KernelSectionKind>,
        start_address: u64,
        end_address_exclusive: u64,
        flags: PageTableFlags,
        hardening: MappingHardening,
        physical_address_bits: u8,
    ) -> Result<IdentityMapping, X86MappingPlanError> {
        let byte_len = end_address_exclusive
            .checked_sub(start_address)
            .ok_or(X86MappingPlanError::ArithmeticOverflow)?;
        let page_count = byte_len / PAGE_SIZE;
        if page_count == 0 || !byte_len.is_multiple_of(PAGE_SIZE) {
            return Err(X86MappingPlanError::ArithmeticOverflow);
        }

        let virtual_page = VirtualPage::new(start_address)
            .ok_or(X86MappingPlanError::NonCanonicalIdentityMapping)?;
        let physical_frame = PhysicalFrame::new(start_address, physical_address_bits)
            .ok_or(X86MappingPlanError::InvalidPhysicalAddress)?;
        let last_start = end_address_exclusive
            .checked_sub(PAGE_SIZE)
            .ok_or(X86MappingPlanError::ArithmeticOverflow)?;
        PhysicalFrame::new(last_start, physical_address_bits)
            .ok_or(X86MappingPlanError::InvalidPhysicalAddress)?;
        VirtualPage::new(last_start).ok_or(X86MappingPlanError::NonCanonicalIdentityMapping)?;

        Ok(IdentityMapping {
            kind,
            kernel_section,
            virtual_page,
            physical_frame,
            page_count,
            flags,
            hardening,
        })
    }

    fn push_kernel_sections(
        &mut self,
        kernel_start: u64,
        kernel_end_exclusive: u64,
        physical_address_bits: u8,
        layout: KernelSectionLayout,
    ) -> Result<(), X86MappingPlanError> {
        let allocation_len = kernel_end_exclusive
            .checked_sub(kernel_start)
            .ok_or(X86MappingPlanError::ArithmeticOverflow)?;
        if layout.text_end_offset() > allocation_len
            || layout.read_only_end_offset() > allocation_len
        {
            return Err(X86MappingPlanError::InvalidKernelSectionLayout);
        }

        let text_end = kernel_start
            .checked_add(layout.text_end_offset())
            .ok_or(X86MappingPlanError::ArithmeticOverflow)?;
        self.push(Self::mapping_for_range(
            PhysicalPreservationKind::KernelImage,
            Some(KernelSectionKind::Text),
            kernel_start,
            text_end,
            PageTableFlags::empty(),
            MappingHardening::Final,
            physical_address_bits,
        )?)?;

        if layout.read_only_end_offset() > layout.text_end_offset() {
            let read_only_end = kernel_start
                .checked_add(layout.read_only_end_offset())
                .ok_or(X86MappingPlanError::ArithmeticOverflow)?;
            self.push(Self::mapping_for_range(
                PhysicalPreservationKind::KernelImage,
                Some(KernelSectionKind::ReadOnlyData),
                text_end,
                read_only_end,
                PageTableFlags::NO_EXECUTE,
                MappingHardening::Final,
                physical_address_bits,
            )?)?;
        }

        if layout.read_only_end_offset() < allocation_len {
            let writable_start = kernel_start
                .checked_add(layout.read_only_end_offset())
                .ok_or(X86MappingPlanError::ArithmeticOverflow)?;
            self.push(Self::mapping_for_range(
                PhysicalPreservationKind::KernelImage,
                Some(KernelSectionKind::WritableData),
                writable_start,
                kernel_end_exclusive,
                PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE),
                MappingHardening::Final,
                physical_address_bits,
            )?)?;
        }

        Ok(())
    }

    fn push(&mut self, mapping: IdentityMapping) -> Result<(), X86MappingPlanError> {
        if self.len >= MAPPINGS {
            return Err(X86MappingPlanError::Capacity);
        }
        self.mappings[self.len] = Some(mapping);
        self.len += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::*;
    use aw_kernel_core::{
        FramebufferHandoff, HandoffPixelFormat, KernelHandoff, KernelImageHandoff,
        MAX_PCIE_ECAM_REGIONS, MemoryDescriptorHandoff, MemoryMapHandoff, PciEcamHandoff,
    };

    fn handoff_with_kernel(
        ecam_base: u64,
        image_byte_len: u64,
        allocation_byte_len: u64,
    ) -> KernelHandoff {
        let mut ecam = [PciEcamHandoff::NONE; MAX_PCIE_ECAM_REGIONS];
        ecam[0] = PciEcamHandoff {
            base_address: ecam_base,
            segment_group: 0,
            start_bus: 0,
            end_bus: 1,
            reserved: 0,
        };

        KernelHandoff::new(
            0x7f00_0123,
            KernelImageHandoff {
                physical_address: 0x40_0000,
                image_byte_len,
                allocation_byte_len,
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

    fn handoff(ecam_base: u64) -> KernelHandoff {
        handoff_with_kernel(ecam_base, 7000, 8192)
    }

    #[test]
    fn translates_preservation_kinds_into_conservative_x86_flags() {
        let preservation = PreservationPlan::<4>::from_handoff(&handoff(0xe000_0000)).unwrap();
        let plan = X86IdentityMappingPlan::<4>::from_preservation(&preservation, 52).unwrap();

        assert_eq!(plan.len(), 4);
        assert!(!plan.is_empty());

        let kernel = plan.get(0).unwrap();
        assert_eq!(kernel.kind(), PhysicalPreservationKind::KernelImage);
        assert_eq!(kernel.kernel_section(), None);
        assert_eq!(kernel.page_count(), 2);
        assert!(kernel.flags().contains(PageTableFlags::WRITABLE));
        assert!(!kernel.flags().contains(PageTableFlags::NO_EXECUTE));
        assert!(kernel.is_writable_executable());
        assert_eq!(
            kernel.hardening(),
            MappingHardening::KernelSectionSplitRequired
        );

        let acpi = plan.get(1).unwrap();
        assert_eq!(acpi.kind(), PhysicalPreservationKind::AcpiRsdp);
        assert_eq!(acpi.kernel_section(), None);
        assert!(acpi.flags().contains(PageTableFlags::NO_EXECUTE));
        assert!(!acpi.flags().contains(PageTableFlags::WRITABLE));
        assert!(!acpi.is_writable_executable());
        assert_eq!(acpi.hardening(), MappingHardening::Final);

        let framebuffer = plan.get(2).unwrap();
        assert_eq!(framebuffer.kind(), PhysicalPreservationKind::Framebuffer);
        assert!(framebuffer.flags().contains(PageTableFlags::WRITABLE));
        assert!(framebuffer.flags().contains(PageTableFlags::NO_EXECUTE));
        assert!(framebuffer.flags().contains(PageTableFlags::CACHE_DISABLE));
        assert!(!framebuffer.is_writable_executable());

        let ecam = plan.get(3).unwrap();
        assert!(matches!(
            ecam.kind(),
            PhysicalPreservationKind::PcieEcam { index: 0 }
        ));
        assert_eq!(ecam.page_count(), 512);
        assert!(ecam.flags().contains(PageTableFlags::WRITABLE));
        assert!(ecam.flags().contains(PageTableFlags::NO_EXECUTE));
        assert!(ecam.flags().contains(PageTableFlags::CACHE_DISABLE));
        assert!(!ecam.is_writable_executable());
        assert_eq!(plan.get(4), None);
    }

    #[test]
    fn sectioned_kernel_plan_is_activation_ready_and_wx_safe() {
        let handoff = handoff_with_kernel(0xe000_0000, 13_000, 16_384);
        let preservation = PreservationPlan::<4>::from_handoff(&handoff).unwrap();
        let layout = KernelSectionLayout::new(4096, 8192).unwrap();
        let plan = X86IdentityMappingPlan::<6>::from_preservation_with_kernel_layout(
            &preservation,
            52,
            layout,
        )
        .unwrap();

        assert_eq!(plan.len(), 6);

        let text = plan.get(0).unwrap();
        assert_eq!(text.kernel_section(), Some(KernelSectionKind::Text));
        assert!(!text.flags().contains(PageTableFlags::WRITABLE));
        assert!(!text.flags().contains(PageTableFlags::NO_EXECUTE));
        assert!(!text.is_writable_executable());
        assert_eq!(text.hardening(), MappingHardening::Final);

        let rodata = plan.get(1).unwrap();
        assert_eq!(
            rodata.kernel_section(),
            Some(KernelSectionKind::ReadOnlyData)
        );
        assert!(!rodata.flags().contains(PageTableFlags::WRITABLE));
        assert!(rodata.flags().contains(PageTableFlags::NO_EXECUTE));

        let writable = plan.get(2).unwrap();
        assert_eq!(
            writable.kernel_section(),
            Some(KernelSectionKind::WritableData)
        );
        assert!(writable.flags().contains(PageTableFlags::WRITABLE));
        assert!(writable.flags().contains(PageTableFlags::NO_EXECUTE));
        assert!(!writable.is_writable_executable());

        let guard = plan.activation_guard().unwrap();
        assert_eq!(guard.mapping_count(), 6);
    }

    #[test]
    fn sectioned_kernel_layout_must_fit_kernel_allocation() {
        let preservation = PreservationPlan::<4>::from_handoff(&handoff(0xe000_0000)).unwrap();
        let layout = KernelSectionLayout::new(4096, 12_288).unwrap();
        assert!(matches!(
            X86IdentityMappingPlan::<6>::from_preservation_with_kernel_layout(
                &preservation,
                52,
                layout
            ),
            Err(X86MappingPlanError::InvalidKernelSectionLayout)
        ));
    }

    #[test]
    fn kernel_section_layout_rejects_unaligned_or_reversed_boundaries() {
        assert_eq!(KernelSectionLayout::new(0, 4096), None);
        assert_eq!(KernelSectionLayout::new(4095, 4096), None);
        assert_eq!(KernelSectionLayout::new(8192, 4096), None);
        assert_eq!(KernelSectionLayout::new(4096, 8193), None);
    }

    #[test]
    fn current_kernel_mapping_cannot_obtain_activation_guard() {
        let preservation = PreservationPlan::<4>::from_handoff(&handoff(0xe000_0000)).unwrap();
        let plan = X86IdentityMappingPlan::<4>::from_preservation(&preservation, 52).unwrap();

        assert!(matches!(
            plan.activation_guard(),
            Err(ActivationGateError::IncompleteHardening { index: 0 })
        ));
    }

    #[test]
    fn activation_gate_rejects_final_writable_executable_mapping() {
        let mut plan = X86IdentityMappingPlan::<1> {
            mappings: [None; 1],
            len: 0,
        };
        plan.push(IdentityMapping {
            kind: PhysicalPreservationKind::KernelImage,
            kernel_section: Some(KernelSectionKind::Text),
            virtual_page: VirtualPage::new(0x40_0000).unwrap(),
            physical_frame: PhysicalFrame::new(0x40_0000, 52).unwrap(),
            page_count: 1,
            flags: PageTableFlags::WRITABLE,
            hardening: MappingHardening::Final,
        })
        .unwrap();

        assert!(matches!(
            plan.activation_guard(),
            Err(ActivationGateError::WritableExecutable { index: 0 })
        ));
    }

    #[test]
    fn activation_gate_accepts_only_final_non_wx_mappings() {
        let mut plan = X86IdentityMappingPlan::<1> {
            mappings: [None; 1],
            len: 0,
        };
        plan.push(IdentityMapping {
            kind: PhysicalPreservationKind::AcpiRsdp,
            kernel_section: None,
            virtual_page: VirtualPage::new(0x7f00_0000).unwrap(),
            physical_frame: PhysicalFrame::new(0x7f00_0000, 52).unwrap(),
            page_count: 1,
            flags: PageTableFlags::NO_EXECUTE,
            hardening: MappingHardening::Final,
        })
        .unwrap();

        let guard = plan.activation_guard().unwrap();
        assert_eq!(guard.mapping_count(), 1);
    }

    #[test]
    fn rejects_identity_mapping_above_canonical_low_half() {
        let preservation =
            PreservationPlan::<4>::from_handoff(&handoff(0x0000_8000_0000_0000)).unwrap();
        assert!(matches!(
            X86IdentityMappingPlan::<4>::from_preservation(&preservation, 52),
            Err(X86MappingPlanError::NonCanonicalIdentityMapping)
        ));
    }

    #[test]
    fn rejects_ranges_outside_reported_physical_width() {
        let preservation = PreservationPlan::<4>::from_handoff(&handoff(0xe000_0000)).unwrap();
        assert!(matches!(
            X86IdentityMappingPlan::<4>::from_preservation(&preservation, 31),
            Err(X86MappingPlanError::InvalidPhysicalAddress)
        ));
    }

    #[test]
    fn rejects_insufficient_mapping_capacity() {
        let preservation = PreservationPlan::<4>::from_handoff(&handoff(0xe000_0000)).unwrap();
        assert!(matches!(
            X86IdentityMappingPlan::<3>::from_preservation(&preservation, 52),
            Err(X86MappingPlanError::Capacity)
        ));
    }
}
