use aw_memory::BootstrapPageAllocator;

use crate::{FrameAllocator, MAX_X86_64_PHYSICAL_ADDRESS_BITS, MappingError, PhysicalFrame};

/// Bridges the generic bootstrap physical-page allocator into the x86-64 paging builder.
///
/// This adapter only hands out page-aligned physical frames. It does not materialize page tables
/// into physical memory, does not modify the active address space and never loads CR3.
pub struct BootstrapFrameAllocator<'allocator, 'map> {
    allocator: &'allocator mut BootstrapPageAllocator<'map>,
    physical_address_bits: u8,
}

impl<'allocator, 'map> BootstrapFrameAllocator<'allocator, 'map> {
    pub fn new(
        allocator: &'allocator mut BootstrapPageAllocator<'map>,
        physical_address_bits: u8,
    ) -> Result<Self, MappingError> {
        if !(12..=MAX_X86_64_PHYSICAL_ADDRESS_BITS).contains(&physical_address_bits) {
            return Err(MappingError::InvalidAddress);
        }

        Ok(Self {
            allocator,
            physical_address_bits,
        })
    }

    #[must_use]
    pub const fn physical_address_bits(&self) -> u8 {
        self.physical_address_bits
    }
}

impl FrameAllocator for BootstrapFrameAllocator<'_, '_> {
    fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
        let page = self.allocator.allocate_page()?;
        PhysicalFrame::new(page.start_address(), self.physical_address_bits)
    }
}

impl<const TABLES: usize> crate::OfflinePageTableBuilder<TABLES> {
    /// Return one deterministic page-table image together with the physical frame reserved for it.
    ///
    /// The returned table is still ordinary Rust memory. This accessor is the safe handoff point
    /// for a later architecture-specific materializer; it never writes physical memory itself.
    #[must_use]
    pub fn table_image(&self, index: usize) -> Option<(PhysicalFrame, &crate::PageTable)> {
        let frame = self.table_frame(index)?;
        let table = self.table_for_frame(frame)?;
        Some((frame, table))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OfflinePageTableBuilder, PageTableFlags, VirtualPage};
    use aw_kernel_core::{MemoryDescriptorHandoff, UEFI_MEMORY_TYPE_CONVENTIONAL};
    use aw_memory::PhysicalRange;

    const fn descriptor(start: u64, page_count: u64) -> MemoryDescriptorHandoff {
        MemoryDescriptorHandoff {
            memory_type: UEFI_MEMORY_TYPE_CONVENTIONAL,
            reserved: 0,
            physical_start: start,
            page_count,
            attributes: 0,
        }
    }

    #[test]
    fn feeds_protected_bootstrap_memory_into_inactive_page_tables() {
        let map = [descriptor(0x20_0000, 8)];
        let protected = [PhysicalRange::new(0x20_1000, 0x20_3000).unwrap()];
        let mut pages = BootstrapPageAllocator::with_protected_ranges(&map, &protected).unwrap();

        {
            let mut frames = BootstrapFrameAllocator::new(&mut pages, 52).unwrap();
            let mut builder = OfflinePageTableBuilder::<4>::new(52, &mut frames).unwrap();

            builder
                .map_4k(
                    &mut frames,
                    VirtualPage::new(0x4000_0000).unwrap(),
                    PhysicalFrame::new(0x80_0000, 52).unwrap(),
                    PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE),
                )
                .unwrap();

            assert_eq!(builder.table_count(), 4);
            assert_eq!(builder.table_frame(0).unwrap().start_address(), 0x20_0000);
            assert_eq!(builder.table_frame(1).unwrap().start_address(), 0x20_3000);
            assert_eq!(builder.table_frame(2).unwrap().start_address(), 0x20_4000);
            assert_eq!(builder.table_frame(3).unwrap().start_address(), 0x20_5000);

            for index in 0..builder.table_count() {
                let (frame, table) = builder.table_image(index).unwrap();
                assert_eq!(frame, builder.table_frame(index).unwrap());
                assert!(core::ptr::eq(
                    table,
                    builder.table_for_frame(frame).unwrap()
                ));
            }
            assert!(builder.table_image(builder.table_count()).is_none());
        }

        assert_eq!(pages.allocated_pages(), 4);
    }

    #[test]
    fn rejects_invalid_reported_physical_address_width() {
        let map = [descriptor(0x20_0000, 1)];
        let mut pages = BootstrapPageAllocator::new(&map).unwrap();

        assert!(matches!(
            BootstrapFrameAllocator::new(&mut pages, 11),
            Err(MappingError::InvalidAddress)
        ));
        assert!(matches!(
            BootstrapFrameAllocator::new(&mut pages, 53),
            Err(MappingError::InvalidAddress)
        ));
        assert_eq!(pages.allocated_pages(), 0);
    }

    #[test]
    fn refuses_frames_outside_the_cpu_physical_address_width() {
        let map = [descriptor(1_u64 << 36, 1)];
        let mut pages = BootstrapPageAllocator::new(&map).unwrap();

        {
            let mut frames = BootstrapFrameAllocator::new(&mut pages, 36).unwrap();
            assert_eq!(frames.allocate_frame(), None);
        }

        assert_eq!(pages.allocated_pages(), 1);
    }
}
