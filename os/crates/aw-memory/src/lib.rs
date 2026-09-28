#![no_std]
#![forbid(unsafe_code)]

use aw_kernel_core::{MemoryDescriptorHandoff, UEFI_MEMORY_TYPE_CONVENTIONAL, UEFI_PAGE_SIZE};

pub const DEFAULT_BOOTSTRAP_MIN_ADDRESS: u64 = 0x10_0000;

const fn is_page_aligned(address: u64) -> bool {
    address & (UEFI_PAGE_SIZE - 1) == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalPage {
    start_address: u64,
}

impl PhysicalPage {
    #[must_use]
    pub const fn start_address(self) -> u64 {
        self.start_address
    }
}

/// Page-aligned physical address interval with an exclusive upper bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRange {
    start_address: u64,
    end_address_exclusive: u64,
}

impl PhysicalRange {
    #[must_use]
    pub const fn new(start_address: u64, end_address_exclusive: u64) -> Option<Self> {
        if start_address >= end_address_exclusive
            || !is_page_aligned(start_address)
            || !is_page_aligned(end_address_exclusive)
        {
            return None;
        }

        Some(Self {
            start_address,
            end_address_exclusive,
        })
    }

    #[must_use]
    pub const fn from_page_count(start_address: u64, page_count: u64) -> Option<Self> {
        if page_count == 0 || !is_page_aligned(start_address) {
            return None;
        }
        let byte_len = match page_count.checked_mul(UEFI_PAGE_SIZE) {
            Some(value) => value,
            None => return None,
        };
        let end = match start_address.checked_add(byte_len) {
            Some(value) => value,
            None => return None,
        };
        Self::new(start_address, end)
    }

    /// Cover an arbitrary non-empty physical byte range with whole UEFI pages.
    #[must_use]
    pub const fn covering_byte_range(start_address: u64, byte_len: u64) -> Option<Self> {
        if byte_len == 0 {
            return None;
        }

        let end_unaligned = match start_address.checked_add(byte_len) {
            Some(value) => value,
            None => return None,
        };
        let aligned_start = start_address & !(UEFI_PAGE_SIZE - 1);
        let aligned_end = if is_page_aligned(end_unaligned) {
            end_unaligned
        } else {
            match end_unaligned.checked_add(UEFI_PAGE_SIZE - 1) {
                Some(value) => value & !(UEFI_PAGE_SIZE - 1),
                None => return None,
            }
        };

        Self::new(aligned_start, aligned_end)
    }

    #[must_use]
    pub const fn start_address(self) -> u64 {
        self.start_address
    }

    #[must_use]
    pub const fn end_address_exclusive(self) -> u64 {
        self.end_address_exclusive
    }

    #[must_use]
    pub const fn contains_address(self, address: u64) -> bool {
        address >= self.start_address && address < self.end_address_exclusive
    }

    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start_address < other.end_address_exclusive
            && other.start_address < self.end_address_exclusive
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapAllocatorError {
    EmptyMemoryMap,
    InvalidDescriptor,
    InvalidMinimumAddress,
    NoConventionalMemory,
}

/// Conservative first-stage physical page allocator.
///
/// The allocator only returns pages from UEFI `CONVENTIONAL` descriptors. It
/// deliberately ignores boot-services, ACPI reclaimable and loader memory even
/// though some of those ranges can be reclaimed later. Explicit protected
/// ranges provide a second safety boundary for kernel, handoff, framebuffer,
/// ECAM/MMIO and future page-table ownership.
///
/// Descriptors and protected ranges do not need to be sorted. Each allocation
/// scans the complete map and selects the lowest unprotected page at or above
/// the monotonically increasing cursor.
pub struct BootstrapPageAllocator<'a> {
    descriptors: &'a [MemoryDescriptorHandoff],
    protected_ranges: &'a [PhysicalRange],
    cursor: u64,
    allocated_pages: u64,
}

impl<'a> BootstrapPageAllocator<'a> {
    pub fn new(
        descriptors: &'a [MemoryDescriptorHandoff],
    ) -> Result<Self, BootstrapAllocatorError> {
        Self::with_minimum_address_and_protected_ranges(
            descriptors,
            DEFAULT_BOOTSTRAP_MIN_ADDRESS,
            &[],
        )
    }

    pub fn with_minimum_address(
        descriptors: &'a [MemoryDescriptorHandoff],
        minimum_address: u64,
    ) -> Result<Self, BootstrapAllocatorError> {
        Self::with_minimum_address_and_protected_ranges(descriptors, minimum_address, &[])
    }

    pub fn with_protected_ranges(
        descriptors: &'a [MemoryDescriptorHandoff],
        protected_ranges: &'a [PhysicalRange],
    ) -> Result<Self, BootstrapAllocatorError> {
        Self::with_minimum_address_and_protected_ranges(
            descriptors,
            DEFAULT_BOOTSTRAP_MIN_ADDRESS,
            protected_ranges,
        )
    }

    pub fn with_minimum_address_and_protected_ranges(
        descriptors: &'a [MemoryDescriptorHandoff],
        minimum_address: u64,
        protected_ranges: &'a [PhysicalRange],
    ) -> Result<Self, BootstrapAllocatorError> {
        if descriptors.is_empty() {
            return Err(BootstrapAllocatorError::EmptyMemoryMap);
        }
        if !minimum_address.is_multiple_of(UEFI_PAGE_SIZE) {
            return Err(BootstrapAllocatorError::InvalidMinimumAddress);
        }

        let mut has_usable_conventional = false;
        for descriptor in descriptors {
            if !descriptor.is_valid() {
                return Err(BootstrapAllocatorError::InvalidDescriptor);
            }
            if descriptor.memory_type == UEFI_MEMORY_TYPE_CONVENTIONAL
                && descriptor
                    .physical_end_exclusive()
                    .is_some_and(|end| end > minimum_address)
            {
                has_usable_conventional = true;
            }
        }

        if !has_usable_conventional {
            return Err(BootstrapAllocatorError::NoConventionalMemory);
        }

        Ok(Self {
            descriptors,
            protected_ranges,
            cursor: minimum_address,
            allocated_pages: 0,
        })
    }

    #[must_use]
    pub const fn allocated_pages(&self) -> u64 {
        self.allocated_pages
    }

    fn next_unprotected_candidate(&self, mut candidate: u64, end: u64) -> Option<u64> {
        loop {
            if candidate >= end {
                return None;
            }

            let mut jump_to: Option<u64> = None;
            for range in self.protected_ranges {
                if range.contains_address(candidate) {
                    jump_to = Some(jump_to.map_or(range.end_address_exclusive(), |current| {
                        current.max(range.end_address_exclusive())
                    }));
                }
            }

            match jump_to {
                Some(next) => candidate = next,
                None => return Some(candidate),
            }
        }
    }

    pub fn allocate_page(&mut self) -> Option<PhysicalPage> {
        let mut best: Option<u64> = None;

        for descriptor in self.descriptors {
            if descriptor.memory_type != UEFI_MEMORY_TYPE_CONVENTIONAL {
                continue;
            }

            let end = descriptor.physical_end_exclusive()?;
            if end <= self.cursor {
                continue;
            }

            let candidate = descriptor.physical_start.max(self.cursor);
            let Some(candidate) = self.next_unprotected_candidate(candidate, end) else {
                continue;
            };

            if best.is_none_or(|current| candidate < current) {
                best = Some(candidate);
            }
        }

        let start_address = best?;
        self.cursor = start_address.checked_add(UEFI_PAGE_SIZE)?;
        self.allocated_pages = self.allocated_pages.checked_add(1)?;
        Some(PhysicalPage { start_address })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn descriptor(memory_type: u32, start: u64, page_count: u64) -> MemoryDescriptorHandoff {
        MemoryDescriptorHandoff {
            memory_type,
            reserved: 0,
            physical_start: start,
            page_count,
            attributes: 0,
        }
    }

    #[test]
    fn physical_ranges_validate_alignment_bounds_and_overflow() {
        assert_eq!(
            PhysicalRange::new(0x20_0000, 0x20_2000),
            Some(PhysicalRange {
                start_address: 0x20_0000,
                end_address_exclusive: 0x20_2000,
            })
        );
        assert!(PhysicalRange::new(0x20_0001, 0x20_2000).is_none());
        assert!(PhysicalRange::new(0x20_0000, 0x20_2001).is_none());
        assert!(PhysicalRange::new(0x20_0000, 0x20_0000).is_none());
        assert!(PhysicalRange::from_page_count(0x20_0000, 0).is_none());
        assert!(PhysicalRange::from_page_count(!(UEFI_PAGE_SIZE - 1), 2).is_none());
    }

    #[test]
    fn physical_range_covers_unaligned_byte_ranges() {
        assert_eq!(
            PhysicalRange::covering_byte_range(0x20_0003, 1),
            PhysicalRange::new(0x20_0000, 0x20_1000)
        );
        assert_eq!(
            PhysicalRange::covering_byte_range(0x20_0fff, 2),
            PhysicalRange::new(0x20_0000, 0x20_2000)
        );
        assert_eq!(
            PhysicalRange::covering_byte_range(0x20_0000, UEFI_PAGE_SIZE),
            PhysicalRange::new(0x20_0000, 0x20_1000)
        );
        assert!(PhysicalRange::covering_byte_range(0x20_0000, 0).is_none());
        assert!(PhysicalRange::covering_byte_range(u64::MAX, 1).is_none());
        assert!(PhysicalRange::covering_byte_range(u64::MAX - 1, 1).is_none());
    }

    #[test]
    fn physical_range_overlap_is_half_open() {
        let first = PhysicalRange::new(0x20_0000, 0x20_2000).unwrap();
        let touching = PhysicalRange::new(0x20_2000, 0x20_3000).unwrap();
        let overlapping = PhysicalRange::new(0x20_1000, 0x20_3000).unwrap();

        assert!(!first.overlaps(touching));
        assert!(first.overlaps(overlapping));
        assert!(overlapping.overlaps(first));
    }

    #[test]
    fn allocates_lowest_pages_even_when_map_is_unsorted() {
        let map = [
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x40_0000, 2),
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x10_0000, 2),
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x20_0000, 1),
        ];
        let mut allocator = BootstrapPageAllocator::new(&map).unwrap();

        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x10_0000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x10_1000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x20_0000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x40_0000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x40_1000
        );
        assert_eq!(allocator.allocate_page(), None);
        assert_eq!(allocator.allocated_pages(), 5);
    }

    #[test]
    fn default_allocator_skips_low_memory() {
        let map = [
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x0, 0x100),
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x10_0000, 1),
        ];
        let mut allocator = BootstrapPageAllocator::new(&map).unwrap();

        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x10_0000
        );
        assert_eq!(allocator.allocate_page(), None);
    }

    #[test]
    fn custom_minimum_address_is_supported() {
        let map = [descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x80_0000, 4)];
        let mut allocator = BootstrapPageAllocator::with_minimum_address(&map, 0x80_2000).unwrap();

        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x80_2000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x80_3000
        );
        assert_eq!(allocator.allocate_page(), None);
    }

    #[test]
    fn protected_range_splits_conventional_memory() {
        let map = [descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x20_0000, 5)];
        let protected = [PhysicalRange::new(0x20_1000, 0x20_3000).unwrap()];
        let mut allocator =
            BootstrapPageAllocator::with_protected_ranges(&map, &protected).unwrap();

        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x20_0000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x20_3000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x20_4000
        );
        assert_eq!(allocator.allocate_page(), None);
        assert_eq!(allocator.allocated_pages(), 3);
    }

    #[test]
    fn overlapping_unsorted_protected_ranges_are_all_skipped() {
        let map = [descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x30_0000, 8)];
        let protected = [
            PhysicalRange::new(0x30_3000, 0x30_6000).unwrap(),
            PhysicalRange::new(0x30_1000, 0x30_4000).unwrap(),
        ];
        let mut allocator =
            BootstrapPageAllocator::with_protected_ranges(&map, &protected).unwrap();

        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x30_0000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x30_6000
        );
        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x30_7000
        );
        assert_eq!(allocator.allocate_page(), None);
    }

    #[test]
    fn fully_protected_conventional_memory_returns_no_page() {
        let map = [descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x40_0000, 2)];
        let protected = [PhysicalRange::new(0x40_0000, 0x40_2000).unwrap()];
        let mut allocator =
            BootstrapPageAllocator::with_protected_ranges(&map, &protected).unwrap();

        assert_eq!(allocator.allocate_page(), None);
        assert_eq!(allocator.allocated_pages(), 0);
    }

    #[test]
    fn ignores_non_conventional_ranges() {
        let map = [
            descriptor(2, 0x10_0000, 8),
            descriptor(11, 0x20_0000, 8),
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x30_0000, 1),
        ];
        let mut allocator = BootstrapPageAllocator::new(&map).unwrap();

        assert_eq!(
            allocator.allocate_page().unwrap().start_address(),
            0x30_0000
        );
        assert_eq!(allocator.allocate_page(), None);
    }

    #[test]
    fn rejects_invalid_descriptors() {
        let map = [MemoryDescriptorHandoff {
            memory_type: UEFI_MEMORY_TYPE_CONVENTIONAL,
            reserved: 0,
            physical_start: 0x10_0001,
            page_count: 1,
            attributes: 0,
        }];

        assert!(matches!(
            BootstrapPageAllocator::new(&map),
            Err(BootstrapAllocatorError::InvalidDescriptor)
        ));
    }

    #[test]
    fn rejects_unaligned_minimum_address() {
        let map = [descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x10_0000, 1)];

        assert!(matches!(
            BootstrapPageAllocator::with_minimum_address(&map, 0x10_0001),
            Err(BootstrapAllocatorError::InvalidMinimumAddress)
        ));
    }

    #[test]
    fn rejects_map_without_usable_conventional_memory() {
        let map = [
            descriptor(2, 0x10_0000, 1),
            descriptor(11, 0x20_0000, 1),
            descriptor(UEFI_MEMORY_TYPE_CONVENTIONAL, 0x0, 1),
        ];

        assert!(matches!(
            BootstrapPageAllocator::new(&map),
            Err(BootstrapAllocatorError::NoConventionalMemory)
        ));
    }

    #[test]
    fn rejects_empty_map() {
        assert!(matches!(
            BootstrapPageAllocator::new(&[]),
            Err(BootstrapAllocatorError::EmptyMemoryMap)
        ));
    }
}
