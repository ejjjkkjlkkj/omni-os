use crate::{
    MappingError, PageTable, PageTableEntry, PageTableFlags, PhysicalFrame, VirtualAddress,
    VirtualPage,
};

/// Size of one 2 MiB leaf mapping.
pub const HUGE_PAGE_2M_SIZE: u64 = 2 * 1024 * 1024;
/// Size of one 1 GiB leaf mapping.
pub const HUGE_PAGE_1G_SIZE: u64 = 1024 * 1024 * 1024;

/// Granularity of a leaf mapping in the hierarchy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeafSize {
    Size4KiB,
    Size2MiB,
    Size1GiB,
}

impl LeafSize {
    #[must_use]
    pub const fn bytes(self) -> u64 {
        match self {
            Self::Size4KiB => crate::PAGE_SIZE,
            Self::Size2MiB => HUGE_PAGE_2M_SIZE,
            Self::Size1GiB => HUGE_PAGE_1G_SIZE,
        }
    }
}

/// A leaf mapping found by [`OfflinePageTableBuilder::resolve`], at whatever
/// granularity it happens to use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedLeaf {
    pub frame: PhysicalFrame,
    pub flags: PageTableFlags,
    pub size: LeafSize,
}

/// Supplies physical frames for page-table storage.
///
/// The allocator remains owned by the caller so the paging crate stays independent from the
/// physical-memory manager implementation.
pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysicalFrame>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedMapping {
    pub frame: PhysicalFrame,
    pub flags: PageTableFlags,
}

/// Safe, inactive four-level x86-64 page-table builder.
///
/// Tables are constructed in ordinary Rust memory and associated with physical frames supplied
/// by the caller. The builder never writes physical memory, never loads CR3 and never changes the
/// active address space. A later kernel integration step can materialize the validated table
/// images at their assigned frames.
pub struct OfflinePageTableBuilder<const TABLES: usize> {
    physical_address_bits: u8,
    root_frame: PhysicalFrame,
    table_frames: [Option<PhysicalFrame>; TABLES],
    tables: [PageTable; TABLES],
    table_count: usize,
}

impl<const TABLES: usize> OfflinePageTableBuilder<TABLES> {
    pub fn new<A: FrameAllocator>(
        physical_address_bits: u8,
        allocator: &mut A,
    ) -> Result<Self, MappingError> {
        if !(12..=crate::MAX_X86_64_PHYSICAL_ADDRESS_BITS).contains(&physical_address_bits) {
            return Err(MappingError::InvalidAddress);
        }
        if TABLES == 0 {
            return Err(MappingError::OutOfFrames);
        }

        let root_frame = allocator
            .allocate_frame()
            .ok_or(MappingError::OutOfFrames)?;
        let root_frame = Self::validate_frame(root_frame, physical_address_bits)?;

        let mut table_frames = [None; TABLES];
        table_frames[0] = Some(root_frame);

        Ok(Self {
            physical_address_bits,
            root_frame,
            table_frames,
            tables: core::array::from_fn(|_| PageTable::new()),
            table_count: 1,
        })
    }

    #[must_use]
    pub const fn root_frame(&self) -> PhysicalFrame {
        self.root_frame
    }

    #[must_use]
    pub const fn table_count(&self) -> usize {
        self.table_count
    }

    #[must_use]
    pub const fn root_table(&self) -> &PageTable {
        &self.tables[0]
    }

    #[must_use]
    pub fn table_frame(&self, index: usize) -> Option<PhysicalFrame> {
        if index >= self.table_count {
            return None;
        }
        self.table_frames[index]
    }

    #[must_use]
    pub fn table_for_frame(&self, frame: PhysicalFrame) -> Option<&PageTable> {
        self.table_index_for_frame(frame)
            .map(|index| &self.tables[index])
    }

    /// Map one canonical 4 KiB virtual page to one physical frame.
    ///
    /// Missing PML4/PDPT/PD/PT tables are allocated lazily. Intermediate entries are always
    /// writable so leaf permissions can remain authoritative. USER is propagated upward only
    /// when required. NX stays a leaf property so executable and non-executable 4 KiB pages can
    /// coexist below the same intermediate tables.
    pub fn map_4k<A: FrameAllocator>(
        &mut self,
        allocator: &mut A,
        page: VirtualPage,
        frame: PhysicalFrame,
        flags: PageTableFlags,
    ) -> Result<(), MappingError> {
        let frame = Self::validate_frame(frame, self.physical_address_bits)?;
        let user_accessible = flags.contains(PageTableFlags::USER_ACCESSIBLE);

        let pdpt = self.ensure_child_table(0, page.pml4_index(), user_accessible, allocator)?;
        let pd = self.ensure_child_table(pdpt, page.pdpt_index(), user_accessible, allocator)?;
        let pt = self.ensure_child_table(pd, page.pd_index(), user_accessible, allocator)?;

        self.tables[pt].map_4k_leaf(page.pt_index(), frame, flags)
    }

    /// Map one 2 MiB region with a PD huge leaf.
    ///
    /// Used for the bulk of an identity window: a 2 MiB leaf costs one entry
    /// instead of 512, which is what makes a full low-memory map affordable at
    /// bootstrap, while still being fine-grained enough to leave holes for the
    /// regions that need 4 KiB permissions.
    pub fn map_2m<A: FrameAllocator>(
        &mut self,
        allocator: &mut A,
        address: VirtualAddress,
        frame: PhysicalFrame,
        flags: PageTableFlags,
    ) -> Result<(), MappingError> {
        self.map_huge(allocator, address, frame, flags, LeafSize::Size2MiB)
    }

    /// Map one 1 GiB region with a PDPT huge leaf.
    pub fn map_1g<A: FrameAllocator>(
        &mut self,
        allocator: &mut A,
        address: VirtualAddress,
        frame: PhysicalFrame,
        flags: PageTableFlags,
    ) -> Result<(), MappingError> {
        self.map_huge(allocator, address, frame, flags, LeafSize::Size1GiB)
    }

    fn map_huge<A: FrameAllocator>(
        &mut self,
        allocator: &mut A,
        address: VirtualAddress,
        frame: PhysicalFrame,
        flags: PageTableFlags,
        size: LeafSize,
    ) -> Result<(), MappingError> {
        let span = size.bytes();
        if !address.value().is_multiple_of(span) || !frame.start_address().is_multiple_of(span) {
            return Err(MappingError::Unaligned);
        }
        let frame = Self::validate_frame(frame, self.physical_address_bits)?;
        let user_accessible = flags.contains(PageTableFlags::USER_ACCESSIBLE);

        let (parent, entry_index) = match size {
            LeafSize::Size1GiB => (
                self.ensure_child_table(0, address.pml4_index(), user_accessible, allocator)?,
                address.pdpt_index(),
            ),
            LeafSize::Size2MiB => {
                let pdpt =
                    self.ensure_child_table(0, address.pml4_index(), user_accessible, allocator)?;
                (
                    self.ensure_child_table(
                        pdpt,
                        address.pdpt_index(),
                        user_accessible,
                        allocator,
                    )?,
                    address.pd_index(),
                )
            }
            LeafSize::Size4KiB => return Err(MappingError::Unaligned),
        };

        let current = self.tables[parent]
            .entry(entry_index)
            .ok_or(MappingError::InvalidAddress)?;
        if current.is_present() {
            return Err(MappingError::AlreadyMapped);
        }

        let leaf = PageTableEntry::from_frame(
            frame,
            flags
                .union(PageTableFlags::PRESENT)
                .union(PageTableFlags::HUGE_PAGE),
        );
        if !self.tables[parent].set_entry(entry_index, leaf) {
            return Err(MappingError::InvalidAddress);
        }
        Ok(())
    }

    /// Resolve whichever leaf covers `address`, at any granularity.
    ///
    /// This is how a mixed-granularity map is audited: the caller can assert
    /// that a given address really is mapped with the permissions it intended,
    /// without having to know whether the mapping came out as a 4 KiB, 2 MiB or
    /// 1 GiB leaf.
    pub fn resolve(&self, address: VirtualAddress) -> Result<ResolvedLeaf, MappingError> {
        let pdpt = self.child_table_index(0, address.pml4_index())?;

        let pdpt_entry = self.tables[pdpt]
            .entry(address.pdpt_index())
            .ok_or(MappingError::InvalidAddress)?;
        if !pdpt_entry.is_present() {
            return Err(MappingError::NotMapped);
        }
        if pdpt_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            return self.leaf(pdpt_entry, LeafSize::Size1GiB);
        }

        let pd = self.child_table_index(pdpt, address.pdpt_index())?;
        let pd_entry = self.tables[pd]
            .entry(address.pd_index())
            .ok_or(MappingError::InvalidAddress)?;
        if !pd_entry.is_present() {
            return Err(MappingError::NotMapped);
        }
        if pd_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            return self.leaf(pd_entry, LeafSize::Size2MiB);
        }

        let pt = self.child_table_index(pd, address.pd_index())?;
        let pt_entry = self.tables[pt]
            .entry(address.pt_index())
            .ok_or(MappingError::InvalidAddress)?;
        if !pt_entry.is_present() {
            return Err(MappingError::NotMapped);
        }
        self.leaf(pt_entry, LeafSize::Size4KiB)
    }

    fn leaf(&self, entry: PageTableEntry, size: LeafSize) -> Result<ResolvedLeaf, MappingError> {
        let frame = PhysicalFrame::new(entry.frame_address(), self.physical_address_bits)
            .ok_or(MappingError::InvalidAddress)?;
        Ok(ResolvedLeaf {
            frame,
            flags: entry.flags(),
            size,
        })
    }

    /// Resolve a 4 KiB mapping from the inactive hierarchy.
    pub fn resolve_4k(&self, page: VirtualPage) -> Result<ResolvedMapping, MappingError> {
        let pdpt = self.child_table_index(0, page.pml4_index())?;
        let pd = self.child_table_index(pdpt, page.pdpt_index())?;
        let pt = self.child_table_index(pd, page.pd_index())?;

        let entry = self.tables[pt]
            .entry(page.pt_index())
            .ok_or(MappingError::InvalidAddress)?;
        if !entry.is_present() {
            return Err(MappingError::NotMapped);
        }

        let frame = PhysicalFrame::new(entry.frame_address(), self.physical_address_bits)
            .ok_or(MappingError::InvalidAddress)?;
        Ok(ResolvedMapping {
            frame,
            flags: entry.flags(),
        })
    }

    fn ensure_child_table<A: FrameAllocator>(
        &mut self,
        parent_table: usize,
        entry_index: usize,
        user_accessible: bool,
        allocator: &mut A,
    ) -> Result<usize, MappingError> {
        let current = self.tables[parent_table]
            .entry(entry_index)
            .ok_or(MappingError::InvalidAddress)?;

        if current.is_present() {
            // A huge leaf already covers this whole sub-tree; walking into it
            // as if it were a table pointer would reinterpret mapped memory as
            // page tables.
            if current.flags().contains(PageTableFlags::HUGE_PAGE) {
                return Err(MappingError::AlreadyMapped);
            }

            let child_frame =
                PhysicalFrame::new(current.frame_address(), self.physical_address_bits)
                    .ok_or(MappingError::InvalidAddress)?;
            let child_index = self
                .table_index_for_frame(child_frame)
                .ok_or(MappingError::InvalidAddress)?;

            if user_accessible && !current.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
                let updated = PageTableEntry::from_frame(
                    child_frame,
                    current.flags().union(PageTableFlags::USER_ACCESSIBLE),
                );
                if !self.tables[parent_table].set_entry(entry_index, updated) {
                    return Err(MappingError::InvalidAddress);
                }
            }

            return Ok(child_index);
        }

        let child_index = self.allocate_table(allocator)?;
        let child_frame = self.table_frames[child_index].ok_or(MappingError::InvalidAddress)?;
        let mut flags = PageTableFlags::PRESENT.union(PageTableFlags::WRITABLE);
        if user_accessible {
            flags = flags.union(PageTableFlags::USER_ACCESSIBLE);
        }

        let entry = PageTableEntry::from_frame(child_frame, flags);
        if !self.tables[parent_table].set_entry(entry_index, entry) {
            return Err(MappingError::InvalidAddress);
        }

        Ok(child_index)
    }

    fn child_table_index(
        &self,
        parent_table: usize,
        entry_index: usize,
    ) -> Result<usize, MappingError> {
        let entry = self.tables[parent_table]
            .entry(entry_index)
            .ok_or(MappingError::InvalidAddress)?;
        if !entry.is_present() {
            return Err(MappingError::NotMapped);
        }

        let frame = PhysicalFrame::new(entry.frame_address(), self.physical_address_bits)
            .ok_or(MappingError::InvalidAddress)?;
        self.table_index_for_frame(frame)
            .ok_or(MappingError::InvalidAddress)
    }

    fn allocate_table<A: FrameAllocator>(
        &mut self,
        allocator: &mut A,
    ) -> Result<usize, MappingError> {
        if self.table_count >= TABLES {
            return Err(MappingError::OutOfFrames);
        }

        let frame = allocator
            .allocate_frame()
            .ok_or(MappingError::OutOfFrames)?;
        let frame = Self::validate_frame(frame, self.physical_address_bits)?;
        if self.table_index_for_frame(frame).is_some() {
            return Err(MappingError::FrameReuse);
        }

        let index = self.table_count;
        self.table_frames[index] = Some(frame);
        self.tables[index] = PageTable::new();
        self.table_count += 1;
        Ok(index)
    }

    fn table_index_for_frame(&self, frame: PhysicalFrame) -> Option<usize> {
        self.table_frames[..self.table_count]
            .iter()
            .position(|candidate| *candidate == Some(frame))
    }

    fn validate_frame(
        frame: PhysicalFrame,
        physical_address_bits: u8,
    ) -> Result<PhysicalFrame, MappingError> {
        PhysicalFrame::new(frame.start_address(), physical_address_bits)
            .ok_or(MappingError::InvalidAddress)
    }
}

#[cfg(test)]
mod mixed_granularity_tests {
    use super::*;

    struct Frames {
        next: u64,
    }

    impl FrameAllocator for Frames {
        fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
            let frame = PhysicalFrame::new(self.next, 52)?;
            self.next += crate::PAGE_SIZE;
            Some(frame)
        }
    }

    fn builder() -> (OfflinePageTableBuilder<8>, Frames) {
        let mut frames = Frames { next: 0x1000_0000 };
        let builder = OfflinePageTableBuilder::<8>::new(52, &mut frames).unwrap();
        (builder, frames)
    }

    fn address(value: u64) -> VirtualAddress {
        VirtualAddress::new(value).unwrap()
    }

    fn frame(value: u64) -> PhysicalFrame {
        PhysicalFrame::new(value, 52).unwrap()
    }

    const RW_NX: PageTableFlags = PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE);

    #[test]
    fn resolves_a_one_gib_leaf() {
        let (mut builder, mut frames) = builder();
        builder
            .map_1g(&mut frames, address(1 << 30), frame(1 << 30), RW_NX)
            .unwrap();

        let leaf = builder.resolve(address((1 << 30) + 0x1234)).unwrap();
        assert_eq!(leaf.size, LeafSize::Size1GiB);
        assert_eq!(leaf.frame.start_address(), 1 << 30);
        assert!(leaf.flags.contains(PageTableFlags::NO_EXECUTE));
        assert!(leaf.flags.contains(PageTableFlags::HUGE_PAGE));
    }

    #[test]
    fn resolves_a_two_mib_leaf() {
        let (mut builder, mut frames) = builder();
        builder
            .map_2m(&mut frames, address(0x40_0000), frame(0x40_0000), RW_NX)
            .unwrap();

        let leaf = builder.resolve(address(0x40_1000)).unwrap();
        assert_eq!(leaf.size, LeafSize::Size2MiB);
        assert_eq!(leaf.frame.start_address(), 0x40_0000);
    }

    #[test]
    fn four_kib_leaves_coexist_with_huge_leaves() {
        let (mut builder, mut frames) = builder();
        // 2 MiB for the bulk, 4 KiB inside a different 2 MiB region.
        builder
            .map_2m(&mut frames, address(0x40_0000), frame(0x40_0000), RW_NX)
            .unwrap();
        builder
            .map_4k(
                &mut frames,
                VirtualPage::new(0x20_1000).unwrap(),
                frame(0x20_1000),
                PageTableFlags::empty(),
            )
            .unwrap();

        assert_eq!(
            builder.resolve(address(0x40_0fff)).unwrap().size,
            LeafSize::Size2MiB
        );
        let text = builder.resolve(address(0x20_1abc)).unwrap();
        assert_eq!(text.size, LeafSize::Size4KiB);
        assert!(!text.flags.contains(PageTableFlags::WRITABLE));
        assert!(!text.flags.contains(PageTableFlags::NO_EXECUTE));
    }

    #[test]
    fn an_unmapped_hole_stays_unmapped() {
        let (mut builder, mut frames) = builder();
        builder
            .map_4k(
                &mut frames,
                VirtualPage::new(0x20_2000).unwrap(),
                frame(0x20_2000),
                RW_NX,
            )
            .unwrap();

        // The guard page next to it was never mapped.
        assert_eq!(
            builder.resolve(address(0x20_1000)),
            Err(MappingError::NotMapped)
        );
    }

    #[test]
    fn rejects_misaligned_huge_mappings() {
        let (mut builder, mut frames) = builder();
        assert_eq!(
            builder.map_2m(&mut frames, address(0x40_1000), frame(0x40_0000), RW_NX),
            Err(MappingError::Unaligned)
        );
        assert_eq!(
            builder.map_1g(&mut frames, address(1 << 30), frame(0x40_0000), RW_NX),
            Err(MappingError::Unaligned)
        );
    }

    #[test]
    fn refuses_to_split_or_overwrite_an_existing_huge_leaf() {
        let (mut builder, mut frames) = builder();
        builder
            .map_2m(&mut frames, address(0x40_0000), frame(0x40_0000), RW_NX)
            .unwrap();

        assert_eq!(
            builder.map_2m(&mut frames, address(0x40_0000), frame(0x40_0000), RW_NX),
            Err(MappingError::AlreadyMapped)
        );
        assert_eq!(
            builder.map_4k(
                &mut frames,
                VirtualPage::new(0x40_0000).unwrap(),
                frame(0x40_0000),
                RW_NX
            ),
            Err(MappingError::AlreadyMapped)
        );
    }

    #[test]
    fn one_gib_and_two_mib_leaves_do_not_collide() {
        let (mut builder, mut frames) = builder();
        builder
            .map_1g(&mut frames, address(1 << 30), frame(1 << 30), RW_NX)
            .unwrap();
        assert_eq!(
            builder.map_2m(
                &mut frames,
                address((1 << 30) + 0x20_0000),
                frame((1 << 30) + 0x20_0000),
                RW_NX
            ),
            Err(MappingError::AlreadyMapped)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestFrameAllocator {
        next: u64,
        remaining: usize,
    }

    impl TestFrameAllocator {
        fn new(remaining: usize) -> Self {
            Self {
                next: 0x1000_0000,
                remaining,
            }
        }
    }

    impl FrameAllocator for TestFrameAllocator {
        fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
            if self.remaining == 0 {
                return None;
            }

            let frame = PhysicalFrame::new(self.next, 52)?;
            self.next = self.next.checked_add(crate::PAGE_SIZE)?;
            self.remaining -= 1;
            Some(frame)
        }
    }

    struct ReusingFrameAllocator {
        frame: PhysicalFrame,
    }

    impl FrameAllocator for ReusingFrameAllocator {
        fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
            Some(self.frame)
        }
    }

    fn leaf_frame(address: u64) -> PhysicalFrame {
        PhysicalFrame::new(address, 52).unwrap()
    }

    #[test]
    fn builds_and_resolves_single_inactive_four_level_mapping() {
        let mut allocator = TestFrameAllocator::new(8);
        let mut builder = OfflinePageTableBuilder::<8>::new(52, &mut allocator).unwrap();
        let page = VirtualPage::new(0x4000_0000).unwrap();
        let frame = leaf_frame(0x2000_0000);
        let flags = PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE);

        assert_eq!(builder.table_count(), 1);
        assert_eq!(builder.map_4k(&mut allocator, page, frame, flags), Ok(()));
        assert_eq!(builder.table_count(), 4);

        let mapping = builder.resolve_4k(page).unwrap();
        assert_eq!(mapping.frame, frame);
        assert!(mapping.flags.contains(PageTableFlags::PRESENT));
        assert!(mapping.flags.contains(PageTableFlags::WRITABLE));
        assert!(mapping.flags.contains(PageTableFlags::NO_EXECUTE));
    }

    #[test]
    fn rejects_duplicate_mapping_without_allocating_more_tables() {
        let mut allocator = TestFrameAllocator::new(8);
        let mut builder = OfflinePageTableBuilder::<8>::new(52, &mut allocator).unwrap();
        let page = VirtualPage::new(0x20_0000).unwrap();

        assert_eq!(
            builder.map_4k(
                &mut allocator,
                page,
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            ),
            Ok(())
        );
        let table_count = builder.table_count();
        assert_eq!(
            builder.map_4k(
                &mut allocator,
                page,
                leaf_frame(0x2000_1000),
                PageTableFlags::WRITABLE,
            ),
            Err(MappingError::AlreadyMapped)
        );
        assert_eq!(builder.table_count(), table_count);
    }

    #[test]
    fn reuses_page_tables_for_multiple_pages_in_the_same_pt() {
        let mut allocator = TestFrameAllocator::new(8);
        let mut builder = OfflinePageTableBuilder::<8>::new(52, &mut allocator).unwrap();
        let first = VirtualPage::new(0x20_0000).unwrap();
        let second = VirtualPage::new(0x20_1000).unwrap();

        builder
            .map_4k(
                &mut allocator,
                first,
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        builder
            .map_4k(
                &mut allocator,
                second,
                leaf_frame(0x2000_1000),
                PageTableFlags::NO_EXECUTE,
            )
            .unwrap();

        assert_eq!(builder.table_count(), 4);
        assert_eq!(
            builder.resolve_4k(first).unwrap().frame,
            leaf_frame(0x2000_0000)
        );
        assert_eq!(
            builder.resolve_4k(second).unwrap().frame,
            leaf_frame(0x2000_1000)
        );
    }

    #[test]
    fn allocates_new_tables_when_crossing_pt_pd_and_pdpt_boundaries() {
        let mut pt_allocator = TestFrameAllocator::new(8);
        let mut pt_builder = OfflinePageTableBuilder::<8>::new(52, &mut pt_allocator).unwrap();
        pt_builder
            .map_4k(
                &mut pt_allocator,
                VirtualPage::new(0x001f_f000).unwrap(),
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        pt_builder
            .map_4k(
                &mut pt_allocator,
                VirtualPage::new(0x0020_0000).unwrap(),
                leaf_frame(0x2000_1000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        assert_eq!(pt_builder.table_count(), 5);

        let mut pd_allocator = TestFrameAllocator::new(8);
        let mut pd_builder = OfflinePageTableBuilder::<8>::new(52, &mut pd_allocator).unwrap();
        pd_builder
            .map_4k(
                &mut pd_allocator,
                VirtualPage::new(0x3fff_f000).unwrap(),
                leaf_frame(0x2100_0000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        pd_builder
            .map_4k(
                &mut pd_allocator,
                VirtualPage::new(0x4000_0000).unwrap(),
                leaf_frame(0x2100_1000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        assert_eq!(pd_builder.table_count(), 6);

        let mut pdpt_allocator = TestFrameAllocator::new(8);
        let mut pdpt_builder = OfflinePageTableBuilder::<8>::new(52, &mut pdpt_allocator).unwrap();
        pdpt_builder
            .map_4k(
                &mut pdpt_allocator,
                VirtualPage::new(0x0000_007f_ffff_f000).unwrap(),
                leaf_frame(0x2200_0000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        pdpt_builder
            .map_4k(
                &mut pdpt_allocator,
                VirtualPage::new(0x0000_0080_0000_0000).unwrap(),
                leaf_frame(0x2200_1000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        assert_eq!(pdpt_builder.table_count(), 7);
    }

    #[test]
    fn propagates_user_permission_through_intermediate_tables() {
        let mut allocator = TestFrameAllocator::new(8);
        let mut builder = OfflinePageTableBuilder::<8>::new(52, &mut allocator).unwrap();
        let supervisor_page = VirtualPage::new(0x20_0000).unwrap();
        let user_page = VirtualPage::new(0x20_1000).unwrap();

        builder
            .map_4k(
                &mut allocator,
                supervisor_page,
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        builder
            .map_4k(
                &mut allocator,
                user_page,
                leaf_frame(0x2000_1000),
                PageTableFlags::USER_ACCESSIBLE,
            )
            .unwrap();

        let pml4_entry = builder.root_table().entry(user_page.pml4_index()).unwrap();
        assert!(pml4_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE));

        let pdpt_frame = PhysicalFrame::new(pml4_entry.frame_address(), 52).unwrap();
        let pdpt = builder.table_for_frame(pdpt_frame).unwrap();
        let pdpt_entry = pdpt.entry(user_page.pdpt_index()).unwrap();
        assert!(pdpt_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE));

        let pd_frame = PhysicalFrame::new(pdpt_entry.frame_address(), 52).unwrap();
        let pd = builder.table_for_frame(pd_frame).unwrap();
        let pd_entry = pd.entry(user_page.pd_index()).unwrap();
        assert!(pd_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE));

        let mapping = builder.resolve_4k(user_page).unwrap();
        assert!(mapping.flags.contains(PageTableFlags::USER_ACCESSIBLE));
        assert!(
            !builder
                .resolve_4k(supervisor_page)
                .unwrap()
                .flags
                .contains(PageTableFlags::USER_ACCESSIBLE)
        );
    }

    #[test]
    fn rejects_allocator_frame_reuse() {
        let frame = leaf_frame(0x1000_0000);
        let mut allocator = ReusingFrameAllocator { frame };
        let mut builder = OfflinePageTableBuilder::<4>::new(52, &mut allocator).unwrap();

        assert_eq!(
            builder.map_4k(
                &mut allocator,
                VirtualPage::new(0x20_0000).unwrap(),
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            ),
            Err(MappingError::FrameReuse)
        );
    }

    #[test]
    fn rejects_out_of_capacity_and_too_wide_leaf_frames() {
        let mut allocator = TestFrameAllocator::new(8);
        let mut builder = OfflinePageTableBuilder::<3>::new(52, &mut allocator).unwrap();
        assert_eq!(
            builder.map_4k(
                &mut allocator,
                VirtualPage::new(0x20_0000).unwrap(),
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            ),
            Err(MappingError::OutOfFrames)
        );

        let mut narrow_allocator = TestFrameAllocator::new(8);
        let mut narrow = OfflinePageTableBuilder::<8>::new(36, &mut narrow_allocator).unwrap();
        let too_wide = PhysicalFrame::new(1_u64 << 40, 52).unwrap();
        assert_eq!(
            narrow.map_4k(
                &mut narrow_allocator,
                VirtualPage::new(0x30_0000).unwrap(),
                too_wide,
                PageTableFlags::WRITABLE,
            ),
            Err(MappingError::InvalidAddress)
        );
    }

    #[test]
    fn never_reuses_allocated_page_table_frames() {
        let mut allocator = TestFrameAllocator::new(8);
        let mut builder = OfflinePageTableBuilder::<8>::new(52, &mut allocator).unwrap();
        builder
            .map_4k(
                &mut allocator,
                VirtualPage::new(0x0000_007f_ffff_f000).unwrap(),
                leaf_frame(0x2000_0000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();
        builder
            .map_4k(
                &mut allocator,
                VirtualPage::new(0x0000_0080_0000_0000).unwrap(),
                leaf_frame(0x2000_1000),
                PageTableFlags::WRITABLE,
            )
            .unwrap();

        for left in 0..builder.table_count() {
            for right in (left + 1)..builder.table_count() {
                assert_ne!(builder.table_frame(left), builder.table_frame(right));
            }
        }
    }

    #[test]
    fn resolve_reports_unmapped_pages() {
        let mut allocator = TestFrameAllocator::new(4);
        let builder = OfflinePageTableBuilder::<4>::new(52, &mut allocator).unwrap();
        assert_eq!(
            builder.resolve_4k(VirtualPage::new(crate::PAGE_SIZE).unwrap()),
            Err(MappingError::NotMapped)
        );
    }
}
