//! Kernel heap: a first-fit free-list behind Rust's global allocator (dossier
//! sections 7 and 10).
//!
//! Everything past the CPU foundations - drivers, a filesystem, user address
//! spaces - needs dynamic allocation. This maps a fixed region above the Ring 3
//! window with the runtime mapper ([`crate::page_mapper`]), backs it with frames
//! from [`crate::frame_allocator`], and hands it out through a first-fit free
//! list wired in as `#[global_allocator]`, so `alloc` (`Box`, `Vec`, ...) is
//! available to the rest of the kernel.
//!
//! Freed blocks are returned to the list but not yet coalesced - a compacting
//! allocator comes with real memory pressure. The list is guarded by a
//! test-and-set spinlock, so it is sound once other CPUs allocate.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use aw_x86_paging::PageTableFlags;

use crate::{debug_write, debug_write_hex_u64, debug_write_u64, frame_allocator, page_mapper};

/// Heap window, above the Ring 3 window (0x2_xxxx_xxxx) so the runtime mapper's
/// walk to it meets only interior tables.
const HEAP_BASE: usize = 0x3_0000_0000;
const HEAP_PAGES: u64 = 512; // 2 MiB
const HEAP_SIZE: usize = HEAP_PAGES as usize * 4096;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// A free block, stored in the free memory it describes.
struct FreeNode {
    size: usize,
    next: Option<&'static mut FreeNode>,
}

impl FreeNode {
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    fn start(&self) -> usize {
        core::ptr::from_ref(self) as usize
    }

    fn end(&self) -> usize {
        self.start() + self.size
    }
}

struct Heap {
    head: FreeNode,
}

impl Heap {
    const fn empty() -> Self {
        Self {
            head: FreeNode::new(0),
        }
    }

    /// Push `[addr, addr+size)` onto the free list.
    ///
    /// # Safety
    /// `addr` must be a writable, otherwise-unused region of at least `size`
    /// bytes, aligned for a `FreeNode`, and `size >= size_of::<FreeNode>()`.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        debug_assert_eq!(align_up(addr, mem::align_of::<FreeNode>()), addr);
        debug_assert!(size >= mem::size_of::<FreeNode>());
        let mut node = FreeNode::new(size);
        node.next = self.head.next.take();
        let node_ptr = addr as *mut FreeNode;
        // SAFETY: the caller guarantees the region is writable and large enough.
        unsafe {
            node_ptr.write(node);
            self.head.next = Some(&mut *node_ptr);
        }
    }

    /// Find and detach the first region that fits `size`/`align`.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut FreeNode, usize)> {
        let mut current = &mut self.head;
        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, size, align) {
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            }
            current = current.next.as_mut().unwrap();
        }
        None
    }

    fn alloc_from_region(region: &FreeNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;
        if alloc_end > region.end() {
            return Err(());
        }
        let excess = region.end() - alloc_end;
        if excess > 0 && excess < mem::size_of::<FreeNode>() {
            // The tail would be too small to hold a free node; skip this region.
            return Err(());
        }
        Ok(alloc_start)
    }

    /// Round a layout up to at least a `FreeNode`'s size and alignment, so every
    /// freed block can hold the list node.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<FreeNode>())
            .expect("alignment overflow")
            .pad_to_align();
        let size = layout.size().max(mem::size_of::<FreeNode>());
        (size, layout.align())
    }
}

pub struct LockedHeap {
    locked: AtomicBool,
    heap: UnsafeCell<Heap>,
}

// SAFETY: every access to the inner Heap is serialised by `locked`.
unsafe impl Sync for LockedHeap {}

impl LockedHeap {
    const fn empty() -> Self {
        Self {
            locked: AtomicBool::new(false),
            heap: UnsafeCell::new(Heap::empty()),
        }
    }

    /// Run `f` with exclusive access to the inner heap, holding the spinlock.
    fn with_heap<R>(&self, f: impl FnOnce(&mut Heap) -> R) -> R {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: the flag grants exclusive access for the duration of `f`.
        let result = f(unsafe { &mut *self.heap.get() });
        self.locked.store(false, Ordering::Release);
        result
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = Heap::size_align(layout);
        self.with_heap(|heap| {
            if let Some((region, alloc_start)) = heap.find_region(size, align) {
                let alloc_end = alloc_start + size;
                let excess = region.end() - alloc_end;
                if excess > 0 {
                    // SAFETY: the tail lies inside the region just detached and
                    // is large enough (checked in alloc_from_region).
                    unsafe { heap.add_free_region(alloc_end, excess) };
                }
                alloc_start as *mut u8
            } else {
                ptr::null_mut()
            }
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (size, _) = Heap::size_align(layout);
        // SAFETY: `ptr`/`size` describe a block this allocator handed out.
        self.with_heap(|heap| unsafe { heap.add_free_region(ptr as usize, size) });
    }
}

/// Map the heap window and hand it to the allocator. Idempotent-safe to call
/// once during bring-up.
///
/// # Safety
/// CPL0, after the kernel owns its page tables and the frame allocator is live.
unsafe fn init() -> bool {
    let flags = PageTableFlags::WRITABLE.union(PageTableFlags::NO_EXECUTE);
    for page in 0..HEAP_PAGES {
        let Some(frame) = frame_allocator::allocate() else {
            return false;
        };
        let va = HEAP_BASE as u64 + page * 4096;
        // SAFETY: the heap window is otherwise unused and the frame is fresh.
        if unsafe { page_mapper::map_page(va, frame, flags) }.is_err() {
            return false;
        }
    }
    // SAFETY: the whole window is now mapped writable and owned by the heap.
    ALLOCATOR.with_heap(|heap| unsafe { heap.add_free_region(HEAP_BASE, HEAP_SIZE) });
    true
}

fn in_heap(addr: u64) -> bool {
    (HEAP_BASE as u64..(HEAP_BASE + HEAP_SIZE) as u64).contains(&addr)
}

/// Prove the heap really allocates, grows, aligns, frees and reuses.
pub fn prove() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    debug_write("AW_HEAP_BEGIN\n");

    // SAFETY: CPL0 bring-up, after the VMM and frame allocator are live.
    if !unsafe { init() } {
        debug_write("AW_HEAP_FAIL reason=init\n");
        return;
    }
    debug_write("AW_HEAP_MAP_OK pages=");
    debug_write_u64(HEAP_PAGES);
    debug_write("\n");

    // A boxed value: written, read back, and living inside the heap window.
    let boxed = Box::new(0x1234_5678_9abc_def0_u64);
    let boxed_addr = core::ptr::from_ref(&*boxed) as u64;
    if *boxed != 0x1234_5678_9abc_def0 || !in_heap(boxed_addr) {
        debug_write("AW_HEAP_FAIL reason=box\n");
        return;
    }
    debug_write("AW_HEAP_BOX_OK addr=");
    debug_write_hex_u64(boxed_addr);
    debug_write("\n");
    drop(boxed);

    // A growing vector: forces reallocation, then a checked reduction.
    let mut values = Vec::new();
    for value in 0..1000_u64 {
        values.push(value);
    }
    let sum: u64 = values.iter().copied().sum();
    if sum != 499_500 || !in_heap(values.as_ptr() as u64) {
        debug_write("AW_HEAP_FAIL reason=vec\n");
        return;
    }
    debug_write("AW_HEAP_VEC_OK sum=");
    debug_write_u64(sum);
    debug_write("\n");
    drop(values);

    // Reuse, in isolation: free a block, then an identical allocation must land
    // on exactly that freed block (nothing else allocates in between).
    let first = Box::new(0_u64);
    let first_addr = core::ptr::from_ref(&*first) as u64;
    drop(first);
    let second = Box::new(0_u64);
    let second_addr = core::ptr::from_ref(&*second) as u64;
    if second_addr != first_addr {
        debug_write("AW_HEAP_FAIL reason=no_reuse\n");
        return;
    }
    drop(second);
    debug_write("AW_HEAP_REUSE_OK addr=");
    debug_write_hex_u64(second_addr);
    debug_write("\n");

    // Alignment: a raw over-aligned allocation must come back aligned.
    let layout = Layout::from_size_align(256, 64).expect("valid layout");
    // SAFETY: non-zero layout; the pointer is freed immediately below.
    let raw = unsafe { alloc::alloc::alloc(layout) };
    if raw.is_null() || !(raw as usize).is_multiple_of(64) {
        debug_write("AW_HEAP_FAIL reason=align\n");
        return;
    }
    // SAFETY: `raw`/`layout` are exactly what `alloc` returned.
    unsafe { alloc::alloc::dealloc(raw, layout) };
    debug_write("AW_HEAP_ALIGN_OK\n");

    debug_write("AW_HEAP_PROOF_OK\n");
}
