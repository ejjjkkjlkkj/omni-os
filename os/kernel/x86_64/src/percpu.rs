//! Per-CPU state, reached through `GS`.
//!
//! Dossier section 8 requires per-CPU structures - tables, run queue, interrupt
//! counters, local logs - and an interrupt handler cannot look its CPU up in a
//! table: it has no argument saying which CPU it is, and reading the APIC ID
//! MSR on every interrupt would put a serialising instruction on the hottest
//! path in the kernel.
//!
//! The architectural answer is a segment base. `IA32_GS_BASE` holds this CPU's
//! block address, and the block stores its own address at offset 0, so
//! `mov rax, gs:[0]` yields a usable pointer in one instruction with no
//! dereference of anything the CPU does not already own.
//!
//! `KERNEL_GS_BASE` and `swapgs` are deliberately not used yet: nothing runs at
//! CPL 3, so `GS` is never under user control. That changes with Ring 3, and
//! the entry path introduced then must swap rather than trust `GS`.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::interrupts::MAX_CPUS;
use crate::local_apic::wrmsr;

const IA32_GS_BASE: u32 = 0xc000_0101;

/// One CPU's own state. `self_address` must stay at offset 0.
#[repr(C, align(64))]
pub struct PerCpu {
    /// This block's own address, so `gs:[0]` is a pointer rather than a base.
    self_address: AtomicU64,
    cpu_index: AtomicU32,
    apic_id: AtomicU32,
    /// Local APIC timer interrupts this CPU has taken.
    timer_ticks: AtomicU64,
    /// Interrupts this CPU has taken from an I/O APIC routed device.
    device_ticks: AtomicU64,
    /// This CPU's IST1 (#DF stack) bounds, so a fault handler can check the
    /// frame landed on *this* CPU's emergency stack. Zero until set.
    ist1_start: AtomicU64,
    ist1_top: AtomicU64,
}

impl PerCpu {
    const fn new() -> Self {
        Self {
            self_address: AtomicU64::new(0),
            cpu_index: AtomicU32::new(u32::MAX),
            apic_id: AtomicU32::new(u32::MAX),
            timer_ticks: AtomicU64::new(0),
            device_ticks: AtomicU64::new(0),
            ist1_start: AtomicU64::new(0),
            ist1_top: AtomicU64::new(0),
        }
    }

    /// The index this CPU was installed under, or [`u32::MAX`] before install.
    #[must_use]
    pub fn cpu_index(&self) -> u32 {
        self.cpu_index.load(Ordering::Relaxed)
    }

    /// The APIC id this CPU reported at install, or [`u32::MAX`] if unknown.
    #[must_use]
    pub fn apic_id(&self) -> u32 {
        self.apic_id.load(Ordering::Relaxed)
    }

    /// Local APIC timer interrupts this CPU has taken.
    #[must_use]
    pub fn timer_ticks(&self) -> u64 {
        self.timer_ticks.load(Ordering::Acquire)
    }

    /// I/O APIC routed device interrupts this CPU has taken.
    #[must_use]
    pub fn device_ticks(&self) -> u64 {
        self.device_ticks.load(Ordering::Acquire)
    }

    /// This CPU's IST1 (#DF stack) bounds `[start, top)`, or [`None`] if unset.
    #[must_use]
    pub fn ist1_bounds(&self) -> Option<(u64, u64)> {
        let start = self.ist1_start.load(Ordering::Acquire);
        let top = self.ist1_top.load(Ordering::Acquire);
        if start == 0 || top == 0 {
            return None;
        }
        Some((start, top))
    }

    /// Record this CPU's IST1 bounds so its own fault handler can range-check the
    /// frame. Written once, by this CPU, during bring-up.
    pub fn set_ist1_bounds(&self, start: u64, top: u64) {
        self.ist1_start.store(start, Ordering::Release);
        self.ist1_top.store(top, Ordering::Release);
    }
}

static PER_CPU: [PerCpu; MAX_CPUS] = [const { PerCpu::new() }; MAX_CPUS];

/// Point this CPU's `GS` at its own block.
///
/// # Safety
///
/// Runs once per CPU at CPL0, with `cpu` a unique index below [`MAX_CPUS`].
pub unsafe fn install(cpu: usize, apic_id: u32) -> bool {
    let Some(block) = PER_CPU.get(cpu) else {
        return false;
    };

    let address = core::ptr::from_ref(block) as u64;
    block.self_address.store(address, Ordering::Relaxed);
    block.cpu_index.store(cpu as u32, Ordering::Relaxed);
    block.apic_id.store(apic_id, Ordering::Relaxed);

    // SAFETY: CPL0. The block is a live static that outlives every CPU, and its
    // self-address was published before `GS` can be used to reach it.
    unsafe { wrmsr(IA32_GS_BASE, address) };
    true
}

/// This CPU's block.
///
/// # Panics
/// Never: a CPU that has not run [`install`] reads a null `gs:[0]`, which is
/// reported as [`None`] rather than dereferenced.
#[must_use]
pub fn current() -> Option<&'static PerCpu> {
    let address: u64;
    // SAFETY: reading a GS-relative qword has no side effects. Offset 0 holds
    // the block's own address, written by `install` before `GS` was set.
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[0]",
            out(reg) address,
            options(nostack, preserves_flags, readonly),
        );
    }
    if address == 0 {
        return None;
    }
    // SAFETY: the value can only have been written by `install`, which stores
    // the address of a `'static` element of `PER_CPU`.
    Some(unsafe { &*(address as *const PerCpu) })
}

/// Read one CPU's block by index, from any CPU.
#[must_use]
pub fn by_index(cpu: usize) -> Option<&'static PerCpu> {
    let block = PER_CPU.get(cpu)?;
    if block.self_address.load(Ordering::Acquire) == 0 {
        return None;
    }
    Some(block)
}

/// Count one local APIC timer interrupt on the current CPU.
pub fn count_timer_tick() {
    if let Some(block) = current() {
        block.timer_ticks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Count one device interrupt on the current CPU.
pub fn count_device_tick() {
    if let Some(block) = current() {
        block.device_ticks.fetch_add(1, Ordering::Relaxed);
    }
}
