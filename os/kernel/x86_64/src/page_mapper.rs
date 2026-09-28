//! Runtime 4 KiB mapping on the live kernel page tables (dossier section 7).
//!
//! [`crate::virtual_memory::activate`] builds the map once, offline, and loads
//! it into CR3. Everything after bring-up - a kernel heap, device MMIO windows,
//! user address spaces - has to add and remove single pages on the tables that
//! are *already* live. This walks the four-level hierarchy from CR3, allocating
//! any missing interior tables from [`crate::frame_allocator`], writes the leaf,
//! and invalidates the one page in the TLB.
//!
//! Every table frame lives in low conventional RAM, which the bring-up map
//! identity-covers read/write, so each table is reachable at its physical
//! address as a `&mut PageTable`.
//!
//! It deliberately refuses an address the low identity window already describes
//! with a 1 GiB or 2 MiB leaf: splitting a huge mapping is a separate operation,
//! and every current caller maps *above* that window, where the walk only ever
//! meets interior tables.

use aw_x86_paging::{
    PageTable, PageTableEntry, PageTableFlags, PhysicalFrame, VirtualAddress,
    MAX_X86_64_PHYSICAL_ADDRESS_BITS,
};

/// Bits [51:12] of a table or leaf entry: the physical frame address.
const PHYS_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
const PAGE_SIZE: u64 = 4096;

const PRESENT: PageTableFlags = PageTableFlags::PRESENT;
const WRITABLE: PageTableFlags = PageTableFlags::WRITABLE;
const HUGE: PageTableFlags = PageTableFlags::HUGE_PAGE;
const USER: PageTableFlags = PageTableFlags::USER_ACCESSIBLE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapError {
    /// The requested virtual or physical address is non-canonical or unaligned.
    BadAddress,
    /// The frame allocator ran dry building an interior table.
    OutOfFrames,
    /// The walk met a 1 GiB or 2 MiB leaf; this mapper only edits 4 KiB leaves.
    HugeLeafInPath,
    /// A 4 KiB leaf is already present at this address.
    AlreadyMapped,
    /// No 4 KiB leaf is present at this address.
    NotMapped,
}

impl MapError {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BadAddress => "bad-address",
            Self::OutOfFrames => "out-of-frames",
            Self::HugeLeafInPath => "huge-leaf-in-path",
            Self::AlreadyMapped => "already-mapped",
            Self::NotMapped => "not-mapped",
        }
    }
}

/// The live top-level page table, from CR3. Its frame is in the identity window,
/// so its physical address doubles as a usable pointer.
fn root_table() -> *mut PageTable {
    (crate::virtual_memory::current_cr3() & PHYS_ADDR_MASK) as *mut PageTable
}

fn physical_frame(address: u64) -> Option<PhysicalFrame> {
    PhysicalFrame::new(address, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
}

/// Invalidate one page's TLB entry after its mapping changed.
fn invlpg(address: u64) {
    // SAFETY: `invlpg` only affects the TLB and faults nothing at CPL0.
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) address, options(nostack, preserves_flags));
    }
}

/// The four table indices a virtual address walks, top level first.
fn walk_indices(address: VirtualAddress) -> [usize; 4] {
    [
        address.pml4_index(),
        address.pdpt_index(),
        address.pd_index(),
        address.pt_index(),
    ]
}

/// Map `phys` at `virt` as a 4 KiB page with `leaf_flags` (the `PRESENT` bit is
/// added here). Interior tables are created on demand; for a user leaf every
/// interior entry on the path is given `USER` so the CPU can walk to it at
/// CPL3.
///
/// # Safety
///
/// CPL0. `phys` must be a free frame the caller owns; mapping it at `virt` makes
/// that address alias the frame until [`unmap_page`] runs.
pub unsafe fn map_page(virt: u64, phys: u64, leaf_flags: PageTableFlags) -> Result<(), MapError> {
    if !virt.is_multiple_of(PAGE_SIZE) || !phys.is_multiple_of(PAGE_SIZE) {
        return Err(MapError::BadAddress);
    }
    let address = VirtualAddress::new(virt).ok_or(MapError::BadAddress)?;
    let indices = walk_indices(address);
    let user = leaf_flags.contains(USER);

    let mut table = root_table();
    // Descend PML4 -> PDPT -> PD, creating interior tables as needed.
    for &index in &indices[..3] {
        // SAFETY: `table` points at an identity-mapped, writable table frame.
        let entry = unsafe { &*table }.entry(index).ok_or(MapError::BadAddress)?;
        let next = if entry.is_present() {
            if entry.flags().contains(HUGE) {
                return Err(MapError::HugeLeafInPath);
            }
            if user && !entry.flags().contains(USER) {
                // A kernel-only interior table now needs to be walkable at CPL3.
                let promoted = PageTableEntry::from_frame(
                    physical_frame(entry.frame_address()).ok_or(MapError::BadAddress)?,
                    entry.flags().union(USER),
                );
                // SAFETY: same live table; index came from `entry` above.
                unsafe { &mut *table }.set_entry(index, promoted);
            }
            entry.frame_address()
        } else {
            let frame = crate::frame_allocator::allocate().ok_or(MapError::OutOfFrames)?;
            // SAFETY: `frame` is a fresh, identity-mapped, writable RAM frame.
            unsafe { core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE as usize) };
            let mut flags = PRESENT.union(WRITABLE);
            if user {
                flags = flags.union(USER);
            }
            let created = PageTableEntry::from_frame(
                physical_frame(frame).ok_or(MapError::BadAddress)?,
                flags,
            );
            // SAFETY: same live table; index is in range.
            unsafe { &mut *table }.set_entry(index, created);
            frame
        };
        table = next as *mut PageTable;
    }

    // `table` is the PT. Refuse to clobber a live leaf.
    let pt_index = indices[3];
    // SAFETY: `table` points at an identity-mapped, writable PT frame.
    if unsafe { &*table }
        .entry(pt_index)
        .ok_or(MapError::BadAddress)?
        .is_present()
    {
        return Err(MapError::AlreadyMapped);
    }
    let leaf = PageTableEntry::from_frame(
        physical_frame(phys).ok_or(MapError::BadAddress)?,
        leaf_flags.union(PRESENT),
    );
    // SAFETY: writing the not-present leaf of a live PT; invalidated below.
    unsafe { &mut *table }.set_entry(pt_index, leaf);
    invlpg(virt);
    Ok(())
}

/// Remove the 4 KiB leaf at `virt`, returning the physical frame it mapped. The
/// interior tables are left in place; only the leaf is cleared.
///
/// # Safety
///
/// CPL0. After this returns the address is unmapped and touching it faults.
pub unsafe fn unmap_page(virt: u64) -> Result<u64, MapError> {
    let address = VirtualAddress::new(virt).ok_or(MapError::BadAddress)?;
    let indices = walk_indices(address);

    let mut table = root_table();
    for &index in &indices[..3] {
        // SAFETY: `table` points at an identity-mapped table frame.
        let entry = unsafe { &*table }.entry(index).ok_or(MapError::BadAddress)?;
        if !entry.is_present() {
            return Err(MapError::NotMapped);
        }
        if entry.flags().contains(HUGE) {
            return Err(MapError::HugeLeafInPath);
        }
        table = entry.frame_address() as *mut PageTable;
    }

    // SAFETY: `table` is the live PT frame.
    let removed = unsafe { &mut *table }
        .unmap_4k_leaf(indices[3])
        .map_err(|_| MapError::NotMapped)?;
    invlpg(virt);
    Ok(removed.frame_address())
}

/// Resolve `virt` to its physical frame and leaf flags, following huge leaves so
/// an address inside the identity window resolves correctly too. Returns `None`
/// if any level on the path is not present.
#[must_use]
pub fn translate(virt: u64) -> Option<(u64, PageTableFlags)> {
    let address = VirtualAddress::new(virt)?;
    let indices = walk_indices(address);

    let mut table = root_table();
    // PML4 and PDPT: a huge leaf can only legally appear at PDPT (1 GiB) or PD
    // (2 MiB), so check for it as the walk descends.
    for (level, &index) in indices[..3].iter().enumerate() {
        // SAFETY: `table` points at an identity-mapped table frame.
        let entry = unsafe { &*table }.entry(index)?;
        if !entry.is_present() {
            return None;
        }
        if entry.flags().contains(HUGE) {
            let offset_mask = match level {
                1 => 0x3fff_ffff_u64, // 1 GiB leaf at PDPT
                2 => 0x001f_ffff_u64, // 2 MiB leaf at PD
                _ => return None,
            };
            return Some((entry.frame_address() + (virt & offset_mask), entry.flags()));
        }
        table = entry.frame_address() as *mut PageTable;
    }

    // SAFETY: `table` is the live PT frame.
    let leaf = unsafe { &*table }.entry(indices[3])?;
    if !leaf.is_present() {
        return None;
    }
    Some((leaf.frame_address(), leaf.flags()))
}
