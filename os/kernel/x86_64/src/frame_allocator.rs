//! Global physical page-frame allocator (dossier section 7).
//!
//! The bootstrap allocator was previously built inside `activate_virtual_memory`,
//! used to back the kernel page tables, and dropped. Everything after bring-up -
//! runtime page mapping, a kernel heap, user address spaces - needs a frame
//! source that outlives that function and, crucially, that never hands back a
//! frame already spent on a live page table. So a single allocator is created
//! here once and kept: the virtual-memory bring-up draws its table frames from
//! it, and [`crate::page_mapper`] draws every later frame from the same cursor,
//! which guarantees the two never collide.
//!
//! It only ever returns UEFI conventional memory above the kernel image, and it
//! is a bump allocator: frames are not returned to it yet. Reclaiming freed
//! frames comes with the real physical allocator; this is the conservative
//! first stage the dossier's roadmap builds on.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

use aw_kernel_core::MemoryDescriptorHandoff;
use aw_memory::{BootstrapPageAllocator, PhysicalRange};

/// A minimal test-and-set spinlock. The bootstrap processor is the only CPU
/// that allocates today - application processors only service their own timer -
/// so this is contention-free in practice, but the lock keeps the global sound
/// the moment a second allocator appears.
struct SpinLock {
    held: AtomicBool,
}

impl SpinLock {
    const fn new() -> Self {
        Self {
            held: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> SpinGuard<'_> {
        while self
            .held
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinGuard { lock: self }
    }
}

struct SpinGuard<'a> {
    lock: &'a SpinLock,
}

impl Drop for SpinGuard<'_> {
    fn drop(&mut self) {
        self.lock.held.store(false, Ordering::Release);
    }
}

static LOCK: SpinLock = SpinLock::new();
static mut ALLOCATOR: Option<BootstrapPageAllocator<'static>> = None;
/// Backing storage for the one protected range (the kernel image), so the
/// allocator can borrow it for `'static`. Written once in [`init`].
static mut PROTECTED: MaybeUninit<[PhysicalRange; 1]> = MaybeUninit::uninit();

/// Build the global allocator from the handoff memory map, protecting the kernel
/// image so its frames are never handed out.
///
/// Returns `false` if it was already initialized or the memory map is unusable.
///
/// # Safety
///
/// CPL0, single core, called exactly once before any [`allocate`].
pub unsafe fn init(
    descriptors: &'static [MemoryDescriptorHandoff],
    kernel_image: PhysicalRange,
) -> bool {
    let _guard = LOCK.lock();

    // SAFETY: guarded by LOCK, and this is the single-core init path.
    unsafe {
        if (*core::ptr::addr_of!(ALLOCATOR)).is_some() {
            return false;
        }

        let protected = (*core::ptr::addr_of_mut!(PROTECTED)).write([kernel_image]);
        // A `'static` view of the protected-range storage above.
        let protected: &'static [PhysicalRange] = core::slice::from_raw_parts(protected.as_ptr(), 1);

        match BootstrapPageAllocator::with_protected_ranges(descriptors, protected) {
            Ok(allocator) => {
                *core::ptr::addr_of_mut!(ALLOCATOR) = Some(allocator);
                true
            }
            Err(_) => false,
        }
    }
}

/// Allocate one physical frame, returning its page-aligned physical address.
#[must_use]
pub fn allocate() -> Option<u64> {
    let _guard = LOCK.lock();

    // SAFETY: guarded by LOCK, so this is the only live access to the allocator.
    unsafe {
        (*core::ptr::addr_of_mut!(ALLOCATOR))
            .as_mut()?
            .allocate_page()
            .map(|page| page.start_address())
    }
}
