#![no_std]
#![forbid(unsafe_code)]

mod memory;
mod offline;

pub use memory::BootstrapFrameAllocator;
pub use offline::{
    FrameAllocator, HUGE_PAGE_1G_SIZE, HUGE_PAGE_2M_SIZE, LeafSize, OfflinePageTableBuilder,
    ResolvedLeaf, ResolvedMapping,
};

pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_TABLE_ENTRIES: usize = 512;
pub const MAX_X86_64_PHYSICAL_ADDRESS_BITS: u8 = 52;
pub const PAGE_FRAME_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualAddress(u64);

impl VirtualAddress {
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        let upper = value >> 48;
        let sign = (value >> 47) & 1;
        if (sign == 0 && upper == 0) || (sign == 1 && upper == 0xffff) {
            Some(Self(value))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn is_page_aligned(self) -> bool {
        self.0 & (PAGE_SIZE - 1) == 0
    }

    #[must_use]
    pub const fn page_offset(self) -> usize {
        (self.0 & (PAGE_SIZE - 1)) as usize
    }

    #[must_use]
    pub const fn pml4_index(self) -> usize {
        ((self.0 >> 39) & 0x1ff) as usize
    }

    #[must_use]
    pub const fn pdpt_index(self) -> usize {
        ((self.0 >> 30) & 0x1ff) as usize
    }

    #[must_use]
    pub const fn pd_index(self) -> usize {
        ((self.0 >> 21) & 0x1ff) as usize
    }

    #[must_use]
    pub const fn pt_index(self) -> usize {
        ((self.0 >> 12) & 0x1ff) as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualPage {
    start_address: VirtualAddress,
}

impl VirtualPage {
    #[must_use]
    pub const fn new(start_address: u64) -> Option<Self> {
        let address = match VirtualAddress::new(start_address) {
            Some(address) => address,
            None => return None,
        };
        if !address.is_page_aligned() {
            return None;
        }
        Some(Self {
            start_address: address,
        })
    }

    #[must_use]
    pub const fn start_address(self) -> VirtualAddress {
        self.start_address
    }

    #[must_use]
    pub const fn next(self) -> Option<Self> {
        let next = match self.start_address.value().checked_add(PAGE_SIZE) {
            Some(next) => next,
            None => return None,
        };
        Self::new(next)
    }

    #[must_use]
    pub const fn pml4_index(self) -> usize {
        self.start_address.pml4_index()
    }

    #[must_use]
    pub const fn pdpt_index(self) -> usize {
        self.start_address.pdpt_index()
    }

    #[must_use]
    pub const fn pd_index(self) -> usize {
        self.start_address.pd_index()
    }

    #[must_use]
    pub const fn pt_index(self) -> usize {
        self.start_address.pt_index()
    }
}

/// Half-open logical range of canonical 4 KiB virtual pages.
///
/// A range may not cross the x86-64 48-bit canonical-address hole. The final
/// page is validated eagerly so later indexing cannot silently produce a
/// non-canonical virtual address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageRange {
    start: VirtualPage,
    page_count: u64,
}

impl PageRange {
    #[must_use]
    pub const fn from_page_count(start: VirtualPage, page_count: u64) -> Option<Self> {
        if page_count == 0 {
            return None;
        }

        let last_offset = match (page_count - 1).checked_mul(PAGE_SIZE) {
            Some(offset) => offset,
            None => return None,
        };
        let last_address = match start.start_address().value().checked_add(last_offset) {
            Some(address) => address,
            None => return None,
        };
        let last = match VirtualPage::new(last_address) {
            Some(page) => page,
            None => return None,
        };

        let start_half = (start.start_address().value() >> 47) & 1;
        let last_half = (last.start_address().value() >> 47) & 1;
        if start_half != last_half {
            return None;
        }

        Some(Self { start, page_count })
    }

    #[must_use]
    pub const fn start(self) -> VirtualPage {
        self.start
    }

    #[must_use]
    pub const fn page_count(self) -> u64 {
        self.page_count
    }

    #[must_use]
    pub const fn page(self, index: u64) -> Option<VirtualPage> {
        if index >= self.page_count {
            return None;
        }
        let offset = match index.checked_mul(PAGE_SIZE) {
            Some(offset) => offset,
            None => return None,
        };
        let address = match self.start.start_address().value().checked_add(offset) {
            Some(address) => address,
            None => return None,
        };
        VirtualPage::new(address)
    }

    #[must_use]
    pub const fn end_address_exclusive(self) -> Option<u64> {
        let byte_len = match self.page_count.checked_mul(PAGE_SIZE) {
            Some(byte_len) => byte_len,
            None => return None,
        };
        self.start.start_address().value().checked_add(byte_len)
    }

    #[must_use]
    pub const fn contains(self, page: VirtualPage) -> bool {
        let start = self.start.start_address().value();
        let address = page.start_address().value();
        if address < start {
            return false;
        }
        let delta = address - start;
        delta / PAGE_SIZE < self.page_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalFrame {
    start_address: u64,
}

impl PhysicalFrame {
    #[must_use]
    pub const fn new(start_address: u64, physical_address_bits: u8) -> Option<Self> {
        if physical_address_bits < 12
            || physical_address_bits > MAX_X86_64_PHYSICAL_ADDRESS_BITS
            || start_address & (PAGE_SIZE - 1) != 0
        {
            return None;
        }

        let limit = 1_u64 << physical_address_bits;
        if start_address >= limit {
            return None;
        }

        Some(Self { start_address })
    }

    #[must_use]
    pub const fn start_address(self) -> u64 {
        self.start_address
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageTableFlags(u64);

impl PageTableFlags {
    pub const PRESENT: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const USER_ACCESSIBLE: Self = Self(1 << 2);
    pub const WRITE_THROUGH: Self = Self(1 << 3);
    pub const CACHE_DISABLE: Self = Self(1 << 4);
    pub const ACCESSED: Self = Self(1 << 5);
    pub const DIRTY: Self = Self(1 << 6);
    pub const HUGE_PAGE: Self = Self(1 << 7);
    pub const GLOBAL: Self = Self(1 << 8);
    pub const NO_EXECUTE: Self = Self(1 << 63);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingError {
    AlreadyMapped,
    NotMapped,
    InvalidAddress,
    Unaligned,
    OutOfFrames,
    Overflow,
    FrameReuse,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn from_frame(frame: PhysicalFrame, flags: PageTableFlags) -> Self {
        Self((frame.start_address() & PAGE_FRAME_ADDRESS_MASK) | flags.bits())
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn frame_address(self) -> u64 {
        self.0 & PAGE_FRAME_ADDRESS_MASK
    }

    #[must_use]
    pub const fn flags(self) -> PageTableFlags {
        PageTableFlags(self.0 & !PAGE_FRAME_ADDRESS_MASK)
    }

    #[must_use]
    pub const fn is_present(self) -> bool {
        self.flags().contains(PageTableFlags::PRESENT)
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; PAGE_TABLE_ENTRIES],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::empty(); PAGE_TABLE_ENTRIES],
        }
    }

    #[must_use]
    pub const fn entry(&self, index: usize) -> Option<PageTableEntry> {
        if index < PAGE_TABLE_ENTRIES {
            Some(self.entries[index])
        } else {
            None
        }
    }

    pub fn set_entry(&mut self, index: usize, entry: PageTableEntry) -> bool {
        if index >= PAGE_TABLE_ENTRIES {
            return false;
        }
        self.entries[index] = entry;
        true
    }

    /// Install one 4 KiB leaf mapping in an already selected PT.
    ///
    /// This deliberately does not allocate or walk higher-level page tables.
    /// It is a safe primitive for the future offline page-table builder.
    pub fn map_4k_leaf(
        &mut self,
        index: usize,
        frame: PhysicalFrame,
        flags: PageTableFlags,
    ) -> Result<(), MappingError> {
        let Some(current) = self.entry(index) else {
            return Err(MappingError::InvalidAddress);
        };
        if current.is_present() {
            return Err(MappingError::AlreadyMapped);
        }

        self.entries[index] =
            PageTableEntry::from_frame(frame, flags.union(PageTableFlags::PRESENT));
        Ok(())
    }

    pub fn unmap_4k_leaf(&mut self, index: usize) -> Result<PageTableEntry, MappingError> {
        let Some(current) = self.entry(index) else {
            return Err(MappingError::InvalidAddress);
        };
        if !current.is_present() {
            return Err(MappingError::NotMapped);
        }

        self.entries[index] = PageTableEntry::empty();
        Ok(current)
    }
}

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Number of 1 GiB entries a single PDPT can hold, i.e. the largest low
/// identity window a single PML4+PDPT pair can describe (512 GiB).
pub const MAX_IDENTITY_GIB: u64 = PAGE_TABLE_ENTRIES as u64;

/// Fill a PML4 and a PDPT with a low identity map built entirely from 1 GiB
/// huge pages: virtual address V maps to physical address V for the first
/// `gib` gibibytes.
///
/// `pml4[0]` is pointed at `pdpt_frame` (the physical frame the caller will
/// materialize `pdpt` into), and `pdpt[0..gib]` become 1 GiB leaves at
/// 0, 1 GiB, 2 GiB, ... Every mapping is supervisor, present and writable;
/// execute permission is left enabled (no NX) so freestanding code anywhere in
/// the window keeps running when the map becomes active.
///
/// This is a pure, inactive builder: it writes only the two `PageTable`s it is
/// given, never physical memory and never CR3. The caller materializes the
/// tables at their frames and loads CR3.
pub fn identity_map_low_gib(
    pml4: &mut PageTable,
    pdpt: &mut PageTable,
    pdpt_frame: PhysicalFrame,
    gib: u64,
) -> Result<(), MappingError> {
    if gib == 0 {
        return Err(MappingError::NotMapped);
    }
    if gib > MAX_IDENTITY_GIB {
        return Err(MappingError::Overflow);
    }

    let link = PageTableFlags::PRESENT.union(PageTableFlags::WRITABLE);
    if !pml4.set_entry(0, PageTableEntry::from_frame(pdpt_frame, link)) {
        return Err(MappingError::InvalidAddress);
    }

    let leaf = PageTableFlags::PRESENT
        .union(PageTableFlags::WRITABLE)
        .union(PageTableFlags::HUGE_PAGE);
    for index in 0..gib {
        let physical = index.checked_mul(1 << 30).ok_or(MappingError::Overflow)?;
        let frame = PhysicalFrame::new(physical, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
            .ok_or(MappingError::InvalidAddress)?;
        if !pdpt.set_entry(index as usize, PageTableEntry::from_frame(frame, leaf)) {
            return Err(MappingError::InvalidAddress);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_map_low_gib_builds_huge_page_leaves() {
        let mut pml4 = PageTable::new();
        let mut pdpt = PageTable::new();
        let pdpt_frame = PhysicalFrame::new(0x10_0000, MAX_X86_64_PHYSICAL_ADDRESS_BITS).unwrap();

        identity_map_low_gib(&mut pml4, &mut pdpt, pdpt_frame, 4).unwrap();

        let root = pml4.entry(0).unwrap();
        assert!(root.is_present());
        assert!(root.flags().contains(PageTableFlags::WRITABLE));
        assert!(!root.flags().contains(PageTableFlags::HUGE_PAGE));
        assert_eq!(root.frame_address(), 0x10_0000);

        for index in 0..4u64 {
            let leaf = pdpt.entry(index as usize).unwrap();
            assert!(leaf.is_present());
            assert!(leaf.flags().contains(PageTableFlags::HUGE_PAGE));
            assert!(leaf.flags().contains(PageTableFlags::WRITABLE));
            assert!(!leaf.flags().contains(PageTableFlags::NO_EXECUTE));
            assert_eq!(leaf.frame_address(), index * (1 << 30));
        }
        assert!(!pdpt.entry(4).unwrap().is_present());
    }

    #[test]
    fn identity_map_low_gib_rejects_bad_window() {
        let mut pml4 = PageTable::new();
        let mut pdpt = PageTable::new();
        let frame = PhysicalFrame::new(0x1000, MAX_X86_64_PHYSICAL_ADDRESS_BITS).unwrap();
        assert_eq!(
            identity_map_low_gib(&mut pml4, &mut pdpt, frame, 0),
            Err(MappingError::NotMapped)
        );
        assert_eq!(
            identity_map_low_gib(&mut pml4, &mut pdpt, frame, MAX_IDENTITY_GIB + 1),
            Err(MappingError::Overflow)
        );
        assert!(identity_map_low_gib(&mut pml4, &mut pdpt, frame, MAX_IDENTITY_GIB).is_ok());
    }

    #[test]
    fn validates_canonical_48_bit_virtual_addresses() {
        assert!(VirtualAddress::new(0x0000_7fff_ffff_ffff).is_some());
        assert!(VirtualAddress::new(0xffff_8000_0000_0000).is_some());
        assert!(VirtualAddress::new(0x0000_8000_0000_0000).is_none());
        assert!(VirtualAddress::new(0xffff_7fff_ffff_ffff).is_none());
    }

    #[test]
    fn decomposes_virtual_address_into_four_level_indices() {
        let address = VirtualAddress::new(0xffff_8123_4567_89ab).unwrap();
        assert_eq!(address.pml4_index(), 0x102);
        assert_eq!(address.pdpt_index(), 0x08d);
        assert_eq!(address.pd_index(), 0x02b);
        assert_eq!(address.pt_index(), 0x078);
        assert_eq!(address.page_offset(), 0x9ab);
    }

    #[test]
    fn validates_virtual_pages_and_preserves_indices() {
        let page = VirtualPage::new(0xffff_8123_4567_8000).unwrap();
        assert_eq!(page.start_address().value(), 0xffff_8123_4567_8000);
        assert_eq!(page.pml4_index(), 0x102);
        assert_eq!(page.pdpt_index(), 0x08d);
        assert_eq!(page.pd_index(), 0x02b);
        assert_eq!(page.pt_index(), 0x078);
        assert!(VirtualPage::new(0xffff_8123_4567_8001).is_none());
        assert!(VirtualPage::new(0x0000_8000_0000_0000).is_none());
    }

    #[test]
    fn virtual_page_next_rejects_noncanonical_transition() {
        let last_low = VirtualPage::new(0x0000_7fff_ffff_f000).unwrap();
        assert_eq!(last_low.next(), None);

        let regular = VirtualPage::new(0x20_0000).unwrap();
        assert_eq!(regular.next().unwrap().start_address().value(), 0x20_1000);
    }

    #[test]
    fn page_ranges_validate_bounds_and_index_pages() {
        let start = VirtualPage::new(0x20_0000).unwrap();
        let range = PageRange::from_page_count(start, 3).unwrap();

        assert_eq!(range.page_count(), 3);
        assert_eq!(range.start(), start);
        assert_eq!(range.page(0), Some(start));
        assert_eq!(range.page(2).unwrap().start_address().value(), 0x20_2000);
        assert_eq!(range.page(3), None);
        assert_eq!(range.end_address_exclusive(), Some(0x20_3000));
        assert!(range.contains(VirtualPage::new(0x20_1000).unwrap()));
        assert!(!range.contains(VirtualPage::new(0x20_3000).unwrap()));
    }

    #[test]
    fn page_ranges_reject_zero_overflow_and_canonical_hole_crossing() {
        let low = VirtualPage::new(0x20_0000).unwrap();
        assert!(PageRange::from_page_count(low, 0).is_none());
        assert!(PageRange::from_page_count(low, u64::MAX).is_none());

        let last_low = VirtualPage::new(0x0000_7fff_ffff_f000).unwrap();
        assert!(PageRange::from_page_count(last_low, 2).is_none());
    }

    #[test]
    fn validates_physical_frames_against_reported_address_width() {
        assert!(PhysicalFrame::new(0x0010_0000, 36).is_some());
        assert!(PhysicalFrame::new(0x0010_0001, 36).is_none());
        assert!(PhysicalFrame::new(1_u64 << 36, 36).is_none());
        assert!(PhysicalFrame::new(0, 11).is_none());
        assert!(PhysicalFrame::new(0, 53).is_none());
    }

    #[test]
    fn encodes_frame_address_and_entry_flags_without_overlap() {
        let frame = PhysicalFrame::new(0x0000_1234_5678_9000, 52).unwrap();
        let flags = PageTableFlags::PRESENT
            .union(PageTableFlags::WRITABLE)
            .union(PageTableFlags::NO_EXECUTE);
        let entry = PageTableEntry::from_frame(frame, flags);

        assert_eq!(entry.frame_address(), 0x0000_1234_5678_9000);
        assert!(entry.is_present());
        assert!(entry.flags().contains(PageTableFlags::WRITABLE));
        assert!(entry.flags().contains(PageTableFlags::NO_EXECUTE));
    }

    #[test]
    fn page_table_is_exactly_one_page_and_page_aligned() {
        assert_eq!(core::mem::size_of::<PageTable>(), PAGE_SIZE as usize);
        assert_eq!(core::mem::align_of::<PageTable>(), PAGE_SIZE as usize);
    }

    #[test]
    fn page_table_bounds_checks_entries() {
        let mut table = PageTable::new();
        let frame = PhysicalFrame::new(0x20_0000, 52).unwrap();
        let entry = PageTableEntry::from_frame(frame, PageTableFlags::PRESENT);

        assert!(table.set_entry(511, entry));
        assert_eq!(table.entry(511), Some(entry));
        assert!(!table.set_entry(512, entry));
        assert_eq!(table.entry(512), None);
    }

    #[test]
    fn maps_and_unmaps_4k_leaf_entries_safely() {
        let mut table = PageTable::new();
        let first = PhysicalFrame::new(0x20_0000, 52).unwrap();
        let second = PhysicalFrame::new(0x30_0000, 52).unwrap();
        let flags = PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE);

        assert_eq!(table.map_4k_leaf(7, first, flags), Ok(()));
        let entry = table.entry(7).unwrap();
        assert_eq!(entry.frame_address(), first.start_address());
        assert!(entry.flags().contains(PageTableFlags::PRESENT));
        assert!(entry.flags().contains(PageTableFlags::WRITABLE));
        assert!(entry.flags().contains(PageTableFlags::NO_EXECUTE));

        assert_eq!(
            table.map_4k_leaf(7, second, PageTableFlags::empty()),
            Err(MappingError::AlreadyMapped)
        );
        assert_eq!(
            table.map_4k_leaf(PAGE_TABLE_ENTRIES, second, PageTableFlags::empty()),
            Err(MappingError::InvalidAddress)
        );

        assert_eq!(table.unmap_4k_leaf(7), Ok(entry));
        assert_eq!(table.entry(7), Some(PageTableEntry::empty()));
        assert_eq!(table.unmap_4k_leaf(7), Err(MappingError::NotMapped));
        assert_eq!(
            table.unmap_4k_leaf(PAGE_TABLE_ENTRIES),
            Err(MappingError::InvalidAddress)
        );
    }
}
