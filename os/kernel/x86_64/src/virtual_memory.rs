//! Kernel virtual memory: take over paging from the firmware with W^X.
//!
//! The freestanding kernel starts out running on the page tables UEFI left
//! active, which map everything writable *and* executable. This module builds a
//! kernel-owned four-level hierarchy that identity-maps the low
//! [`IDENTITY_GIB`] GiB and then loads it into CR3.
//!
//! Identity mapping keeps every physical address at the same virtual address,
//! so the currently executing code, the active stack, the framebuffer and the
//! PCIe ECAM window all stay valid across the switch. What changes is
//! *permissions*, at mixed granularity:
//!
//! | region                              | leaf   | permissions |
//! |-------------------------------------|--------|-------------|
//! | everything outside the kernel image | 1G / 2M | RW, NX      |
//! | `.awhdr` + `.rodata`                | 4K     | RO, NX      |
//! | `.text`                             | 4K     | RO, **X**   |
//! | `.data` + `.bss`                    | 4K     | RW, NX      |
//! | IST stack guard page                | -      | not present |
//!
//! No region is ever simultaneously writable and executable, which is the W^X
//! invariant the dossier requires as P0 (section 7). The claim is not made on
//! the strength of the flags alone: [`crate::memory_protection`] deliberately
//! faults against each of them after the map is active.

use core::arch::asm;
use core::arch::x86_64::__cpuid;
use core::mem::MaybeUninit;

use aw_x86_paging::{
    FrameAllocator, LeafSize, MAX_X86_64_PHYSICAL_ADDRESS_BITS, MappingError,
    OfflinePageTableBuilder, PAGE_SIZE, PageTable, PageTableFlags, PhysicalFrame, VirtualAddress,
    VirtualPage,
};

/// Size of the low identity window installed at bring-up. 4 GiB covers all
/// conventional RAM, the framebuffer and the PCIe ECAM region on the supported
/// platforms.
pub const IDENTITY_GIB: u64 = 4;

const GIB: u64 = 1 << 30;
const TWO_MIB: u64 = 2 * 1024 * 1024;

/// Root plus enough sparse child tables for the low bootstrap window and every
/// framebuffer/PCIe ECAM range handed off by firmware. 40 tables cost 160 KiB of
/// BSS and cover the worst case of four disjoint ECAM regions plus framebuffer
/// and xHCI MMIO ranges even when they cross PML4/PDPT boundaries.
const PAGE_TABLE_CAPACITY: usize = 40;

/// Number of 8-byte entries in one page table.
const PAGE_TABLE_ENTRIES: usize = 512;

unsafe extern "C" {
    static __image_start: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __bss_end: u8;
}

fn symbol_address(symbol: &u8) -> u64 {
    core::ptr::from_ref(symbol) as u64
}

/// Physical extent of the loaded kernel image, section by section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelImageLayout {
    /// `.awhdr`: the `AWKN` header page, read-only data.
    pub header: (u64, u64),
    /// `.text`: the only executable range in the whole address space.
    pub text: (u64, u64),
    /// `.rodata`: read-only, never executable.
    pub rodata: (u64, u64),
    /// `.data` + `.bss`: writable, never executable.
    pub data: (u64, u64),
}

impl KernelImageLayout {
    /// Read the layout from the symbols the linker script exports.
    #[must_use]
    pub fn current() -> Self {
        // SAFETY: these are linker-defined symbols; only their addresses are
        // taken, never their (nonexistent) contents.
        unsafe {
            Self {
                header: (
                    symbol_address(&__image_start),
                    symbol_address(&__text_start),
                ),
                text: (symbol_address(&__text_start), symbol_address(&__text_end)),
                rodata: (
                    symbol_address(&__rodata_start),
                    symbol_address(&__rodata_end),
                ),
                data: (symbol_address(&__data_start), symbol_address(&__bss_end)),
            }
        }
    }

    #[must_use]
    pub const fn start(&self) -> u64 {
        self.header.0
    }

    #[must_use]
    pub const fn end(&self) -> u64 {
        self.data.1
    }

    fn is_well_formed(&self) -> bool {
        let bounds = [self.header, self.text, self.rodata, self.data];
        let mut previous = 0;
        for (start, end) in bounds {
            if start >= end
                || !start.is_multiple_of(PAGE_SIZE)
                || !end.is_multiple_of(PAGE_SIZE)
                || start < previous
            {
                return false;
            }
            previous = end;
        }
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmmError {
    /// The CPU does not advertise 1 GiB pages (CPUID.80000001H:EDX[26]).
    NoOneGibPages,
    /// The linker-exported section bounds are missing, unordered or unaligned.
    BadImageLayout,
    /// The physical-frame source ran dry or handed back an unusable frame.
    BadTableFrame,
    /// A page-table frame landed outside the identity window, so it would stop
    /// being reachable the moment the new map goes live.
    TableFrameOutsideWindow,
    /// The pure builder rejected a mapping request.
    BuildFailed,
    /// The built map does not have the permissions it was asked for.
    AuditFailed,
}

impl VmmError {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoOneGibPages => "no-1gib-pages",
            Self::BadImageLayout => "bad-image-layout",
            Self::BadTableFrame => "bad-table-frame",
            Self::TableFrameOutsideWindow => "table-frame-outside-window",
            Self::BuildFailed => "build-failed",
            Self::AuditFailed => "audit-failed",
        }
    }
}

/// Whether the CPU supports 1 GiB pages (CPUID.80000001H:EDX bit 26).
#[must_use]
pub fn supports_1gib_pages() -> bool {
    // Extended leaf 0x8000_0001 is architectural on every x86-64 CPU this
    // kernel targets; `__cpuid` is safe on this always-available target.
    let leaf = __cpuid(0x8000_0001);
    (leaf.edx & (1 << 26)) != 0
}

/// Read the active CR3 (physical address of the top-level page table).
#[must_use]
pub fn current_cr3() -> u64 {
    let value: u64;
    // SAFETY: reading CR3 at CPL0 has no side effects.
    unsafe {
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Adapts a physical-page source to the paging crate's allocator trait, and
/// rejects any frame that would fall outside the identity window.
struct WindowedFrames<'a, F: FnMut() -> Option<u64>> {
    next: &'a mut F,
    escaped_window: bool,
}

impl<F: FnMut() -> Option<u64>> FrameAllocator for WindowedFrames<'_, F> {
    fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
        let address = (self.next)()?;
        if address >= IDENTITY_GIB * GIB {
            self.escaped_window = true;
            return None;
        }
        PhysicalFrame::new(address, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
    }
}

const RW_NX: PageTableFlags = PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE);
const RO_NX: PageTableFlags = PageTableFlags::NO_EXECUTE;
const RO_EXEC: PageTableFlags = PageTableFlags::empty();

/// Where the built hierarchy lives until it is materialized into its frames.
///
/// `OfflinePageTableBuilder` holds one full 4 KiB `PageTable` per table, which
/// is far too much for the bootstrap stack, so it is parked in BSS. It is used
/// exactly once, during single-core bring-up.
static mut KERNEL_PAGE_TABLES: MaybeUninit<OfflinePageTableBuilder<PAGE_TABLE_CAPACITY>> =
    MaybeUninit::uninit();

/// Result of a successful [`activate`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveMap {
    pub previous_cr3: u64,
    pub cr3: u64,
    pub table_count: usize,
    pub layout: KernelImageLayout,
    pub guard_page: u64,
    pub extra_identity_range_count: usize,
}

/// Build the W^X identity map, audit it, and load it into CR3.
///
/// `next_frame` yields free, page-aligned physical frames for page-table
/// storage; it must not hand back memory that is in use, and in particular not
/// the kernel image itself.
///
/// `guard_page` is a page-aligned address inside the kernel image that is
/// deliberately left unmapped, so a stack overflow past it faults instead of
/// silently corrupting the neighbouring data.
///
/// # Safety
///
/// Must run once at CPL0 during single-core bootstrap, with interrupts masked.
/// On success the kernel is executing on kernel-owned page tables and the
/// firmware tables are no longer referenced.
pub unsafe fn activate(
    mut next_frame: impl FnMut() -> Option<u64>,
    guard_page: u64,
    extra_identity_ranges: &[(u64, u64)],
) -> Result<ActiveMap, VmmError> {
    // 1 GiB leaves are an optimization, not a correctness requirement.
    // Older x86-64 CPUs such as Westmere lack CPUID.80000001H:EDX[26];
    // build the same audited identity map from 2 MiB leaves on those CPUs.
    let one_gib_pages = supports_1gib_pages();

    let layout = KernelImageLayout::current();
    if !layout.is_well_formed() || !guard_page.is_multiple_of(PAGE_SIZE) {
        return Err(VmmError::BadImageLayout);
    }

    let mut frames = WindowedFrames {
        next: &mut next_frame,
        escaped_window: false,
    };

    let mut builder = OfflinePageTableBuilder::<PAGE_TABLE_CAPACITY>::new(
        MAX_X86_64_PHYSICAL_ADDRESS_BITS,
        &mut frames,
    )
    .map_err(|error| frame_error(error, frames.escaped_window))?;

    build_map(
        &mut builder,
        &mut frames,
        layout,
        guard_page,
        extra_identity_ranges,
        one_gib_pages,
    )?;
    audit_map(&builder, layout, guard_page, extra_identity_ranges)?;

    let root = builder.root_frame().start_address();
    let table_count = builder.table_count();
    let previous_cr3 = current_cr3();

    // Park the built hierarchy in BSS so it can be materialized without a
    // 32 KiB stack copy, then write each table to its assigned frame.
    // SAFETY: single-core bootstrap, called once; nothing else touches this
    // static, and the reference does not outlive the loop below.
    let parked: &OfflinePageTableBuilder<PAGE_TABLE_CAPACITY> =
        unsafe { (*core::ptr::addr_of_mut!(KERNEL_PAGE_TABLES)).write(builder) };

    for index in 0..table_count {
        let frame = parked
            .table_frame(index)
            .ok_or(VmmError::BadTableFrame)?
            .start_address();
        let table = parked
            .table_for_frame(
                PhysicalFrame::new(frame, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
                    .ok_or(VmmError::BadTableFrame)?,
            )
            .ok_or(VmmError::BadTableFrame)?;
        // SAFETY: the frame came from the caller's free-page source, is page
        // aligned, is inside the identity window and is still reachable at its
        // physical address through the firmware map that is active right now.
        unsafe { materialize_table(frame, table) };
    }

    // SAFETY: the root table is fully populated and identity-maps the currently
    // executing code, the current stack and every table frame, so execution
    // continues uninterrupted across the CR3 write. Writing CR3 also flushes
    // the TLB.
    unsafe { asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags)) };

    Ok(ActiveMap {
        previous_cr3,
        cr3: root,
        table_count,
        layout,
        guard_page,
        extra_identity_range_count: extra_identity_ranges.len(),
    })
}

fn frame_error(error: MappingError, escaped_window: bool) -> VmmError {
    match error {
        MappingError::OutOfFrames if escaped_window => VmmError::TableFrameOutsideWindow,
        MappingError::OutOfFrames | MappingError::FrameReuse => VmmError::BadTableFrame,
        _ => VmmError::BuildFailed,
    }
}

fn build_map<F: FnMut() -> Option<u64>>(
    builder: &mut OfflinePageTableBuilder<PAGE_TABLE_CAPACITY>,
    frames: &mut WindowedFrames<'_, F>,
    layout: KernelImageLayout,
    guard_page: u64,
    extra_identity_ranges: &[(u64, u64)],
    one_gib_pages: bool,
) -> Result<(), VmmError> {
    let image_start = layout.start();
    let image_end = layout.end();

    let overlaps_image =
        |start: u64, span: u64| start < image_end && start.saturating_add(span) > image_start;

    // Everything outside the kernel image: the largest CPU-supported leaf that
    // fits, always RW and never executable. When 1 GiB pages are unavailable,
    // the same low identity window is built entirely from 2 MiB leaves. A huge
    // region containing the image is broken down and its image-bearing 2 MiB
    // span is left for the 4 KiB pass below.
    let mut address = 0;
    while address < IDENTITY_GIB * GIB {
        if one_gib_pages && address.is_multiple_of(GIB) && !overlaps_image(address, GIB) {
            map_huge(builder, frames, address, LeafSize::Size1GiB)?;
            address += GIB;
            continue;
        }

        if !overlaps_image(address, TWO_MIB) {
            map_huge(builder, frames, address, LeafSize::Size2MiB)?;
        }
        address += TWO_MIB;
    }

    // The 2 MiB region(s) the image lands in, one 4 KiB leaf at a time. Every
    // page in the span is mapped, not only the image's own sections: the tail
    // between the image and the 2 MiB boundary (and any gap between sections) is
    // conventional RAM the frame allocator will hand out, so it must be part of
    // the identity map or a table frame placed there would fault when zeroed.
    // Image sections keep their W^X permissions; the filler is RW, never
    // executable. The guard page is skipped so it stays unmapped.
    let region_start = image_start & !(TWO_MIB - 1);
    let region_end = image_end
        .checked_add(TWO_MIB - 1)
        .ok_or(VmmError::BuildFailed)?
        & !(TWO_MIB - 1);
    let mut page = region_start;
    while page < region_end {
        if page != guard_page {
            let flags = image_page_flags(page, layout);
            let virtual_page = VirtualPage::new(page).ok_or(VmmError::BuildFailed)?;
            let frame = PhysicalFrame::new(page, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
                .ok_or(VmmError::BuildFailed)?;
            builder
                .map_4k(frames, virtual_page, frame, flags)
                .map_err(|error| frame_error(error, frames.escaped_window))?;
        }
        page += PAGE_SIZE;
    }

    map_extra_identity_ranges(builder, frames, extra_identity_ranges, one_gib_pages)?;
    Ok(())
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|rounded| rounded & !mask)
}

fn map_extra_identity_ranges<F: FnMut() -> Option<u64>>(
    builder: &mut OfflinePageTableBuilder<PAGE_TABLE_CAPACITY>,
    frames: &mut WindowedFrames<'_, F>,
    ranges: &[(u64, u64)],
    one_gib_pages: bool,
) -> Result<(), VmmError> {
    let low_end = IDENTITY_GIB * GIB;

    for &(start, end) in ranges {
        if start >= end {
            return Err(VmmError::BuildFailed);
        }

        let mut address = (start & !(PAGE_SIZE - 1)).max(low_end);
        let end = align_up(end, PAGE_SIZE).ok_or(VmmError::BuildFailed)?;
        if address >= end {
            continue;
        }

        while address < end {
            let virtual_address = VirtualAddress::new(address).ok_or(VmmError::BuildFailed)?;

            // Ranges may overlap each other. Reuse an existing identity leaf only
            // when it already has the required MMIO permissions.
            if let Ok(leaf) = builder.resolve(virtual_address) {
                let span = leaf.size.bytes();
                let leaf_start = address & !(span - 1);
                if leaf.frame.start_address() != leaf_start
                    || !leaf.flags.contains(PageTableFlags::WRITABLE)
                    || !leaf.flags.contains(PageTableFlags::NO_EXECUTE)
                    || leaf.flags.contains(PageTableFlags::USER_ACCESSIBLE)
                {
                    return Err(VmmError::BuildFailed);
                }
                address = leaf_start.checked_add(span).ok_or(VmmError::BuildFailed)?;
                continue;
            }

            let remaining = end - address;
            if one_gib_pages && address.is_multiple_of(GIB) && remaining >= GIB {
                map_huge(builder, frames, address, LeafSize::Size1GiB)?;
                address += GIB;
            } else if address.is_multiple_of(TWO_MIB) && remaining >= TWO_MIB {
                map_huge(builder, frames, address, LeafSize::Size2MiB)?;
                address += TWO_MIB;
            } else {
                let page = VirtualPage::new(address).ok_or(VmmError::BuildFailed)?;
                let frame = PhysicalFrame::new(address, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
                    .ok_or(VmmError::BuildFailed)?;
                builder
                    .map_4k(frames, page, frame, RW_NX)
                    .map_err(|error| frame_error(error, frames.escaped_window))?;
                address += PAGE_SIZE;
            }
        }
    }

    Ok(())
}

/// Permissions for one 4 KiB page of the image's 2 MiB region: `.text` stays
/// executable and read-only, `.awhdr`/`.rodata` read-only, and everything else -
/// `.data`/`.bss`, section gaps and the tail up to the 2 MiB boundary - is
/// writable and never executable. Never writable-and-executable (W^X).
fn image_page_flags(page: u64, layout: KernelImageLayout) -> PageTableFlags {
    let in_range = |range: (u64, u64)| range.0 <= page && page < range.1;
    if in_range(layout.text) {
        RO_EXEC
    } else if in_range(layout.header) || in_range(layout.rodata) {
        RO_NX
    } else {
        RW_NX
    }
}

fn map_huge<F: FnMut() -> Option<u64>>(
    builder: &mut OfflinePageTableBuilder<PAGE_TABLE_CAPACITY>,
    frames: &mut WindowedFrames<'_, F>,
    address: u64,
    size: LeafSize,
) -> Result<(), VmmError> {
    let virtual_address = VirtualAddress::new(address).ok_or(VmmError::BuildFailed)?;
    let frame = PhysicalFrame::new(address, MAX_X86_64_PHYSICAL_ADDRESS_BITS)
        .ok_or(VmmError::BuildFailed)?;
    let result = match size {
        LeafSize::Size1GiB => builder.map_1g(frames, virtual_address, frame, RW_NX),
        _ => builder.map_2m(frames, virtual_address, frame, RW_NX),
    };
    result.map_err(|error| frame_error(error, frames.escaped_window))
}

/// Re-read the built hierarchy and confirm it says what it was asked to say.
///
/// A permission bug here would not fail the boot - it would silently give back
/// the writable-and-executable world the map exists to remove - so the
/// invariant is checked before CR3 is ever pointed at these tables.
fn audit_map(
    builder: &OfflinePageTableBuilder<PAGE_TABLE_CAPACITY>,
    layout: KernelImageLayout,
    guard_page: u64,
    extra_identity_ranges: &[(u64, u64)],
) -> Result<(), VmmError> {
    let expectations = [
        (layout.header, false, false),
        (layout.text, false, true),
        (layout.rodata, false, false),
        (layout.data, true, false),
    ];

    for ((start, end), writable, executable) in expectations {
        for probe in [start, end - PAGE_SIZE] {
            if probe == guard_page {
                continue;
            }
            let address = VirtualAddress::new(probe).ok_or(VmmError::AuditFailed)?;
            let leaf = builder
                .resolve(address)
                .map_err(|_| VmmError::AuditFailed)?;
            if leaf.size != LeafSize::Size4KiB
                || leaf.frame.start_address() != probe
                || leaf.flags.contains(PageTableFlags::WRITABLE) != writable
                || leaf.flags.contains(PageTableFlags::NO_EXECUTE) == executable
                || leaf.flags.contains(PageTableFlags::USER_ACCESSIBLE)
            {
                return Err(VmmError::AuditFailed);
            }
        }
    }

    // The guard page must have no mapping at all.
    let guard = VirtualAddress::new(guard_page).ok_or(VmmError::AuditFailed)?;
    if builder.resolve(guard).is_ok() {
        return Err(VmmError::AuditFailed);
    }

    // And nothing outside the image may be executable.
    for probe in [0x1000, TWO_MIB, GIB, (IDENTITY_GIB - 1) * GIB] {
        let address = VirtualAddress::new(probe).ok_or(VmmError::AuditFailed)?;
        let leaf = builder
            .resolve(address)
            .map_err(|_| VmmError::AuditFailed)?;
        if !leaf.flags.contains(PageTableFlags::NO_EXECUTE) {
            return Err(VmmError::AuditFailed);
        }
    }

    // Firmware-owned MMIO may sit above 4 GiB. Verify the first and last page of
    // every handed-off range are present, identity-mapped, writable and NX.
    let low_end = IDENTITY_GIB * GIB;
    for &(start, end) in extra_identity_ranges {
        if start >= end {
            return Err(VmmError::AuditFailed);
        }
        let first = (start & !(PAGE_SIZE - 1)).max(low_end);
        let rounded_end = align_up(end, PAGE_SIZE).ok_or(VmmError::AuditFailed)?;
        if first >= rounded_end {
            continue;
        }
        for probe in [first, rounded_end - PAGE_SIZE] {
            let address = VirtualAddress::new(probe).ok_or(VmmError::AuditFailed)?;
            let leaf = builder
                .resolve(address)
                .map_err(|_| VmmError::AuditFailed)?;
            let span = leaf.size.bytes();
            let leaf_start = probe & !(span - 1);
            if leaf.frame.start_address() != leaf_start
                || !leaf.flags.contains(PageTableFlags::WRITABLE)
                || !leaf.flags.contains(PageTableFlags::NO_EXECUTE)
                || leaf.flags.contains(PageTableFlags::USER_ACCESSIBLE)
            {
                return Err(VmmError::AuditFailed);
            }
        }
    }

    Ok(())
}

/// Copy a built page table into its physical frame as 512 raw 64-bit entries.
///
/// # Safety
/// `frame` must be a 4 KiB-aligned writable RAM frame reachable at its physical
/// address (true while the firmware identity map is still active).
unsafe fn materialize_table(frame: u64, table: &PageTable) {
    let destination = frame as *mut u64;
    let mut index = 0;
    while index < PAGE_TABLE_ENTRIES {
        let raw = match table.entry(index) {
            Some(entry) => entry.raw(),
            None => 0,
        };
        // SAFETY: `index` stays below 512, so the write lands inside the frame.
        unsafe {
            core::ptr::write_volatile(destination.add(index), raw);
        }
        index += 1;
    }
}
