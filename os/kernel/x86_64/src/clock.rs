//! Monotonic time from the invariant TSC, calibrated with the PIT (dossier
//! section 8).
//!
//! Section 8 asks for a monotonic clock from the invariant TSC, calibrated from
//! the PIT/APIC as a fallback. This reads the timestamp counter, proves it
//! advances monotonically, and derives its frequency by timing a fixed PIT
//! channel-2 one-shot - giving the kernel a real time base for scheduling and
//! timeouts. It polls the PIT, so it needs no interrupts.

use crate::pit::PIT_INPUT_HZ;
use crate::{debug_write, debug_write_u64};

/// Speaker/PIT control port; bit 0 gates channel 2, bit 5 reflects its output.
const PORT_61: u16 = 0x61;
const PIT_MODE_PORT: u16 = 0x43;
const PIT_CH2_PORT: u16 = 0x42;

/// Calibration window: 10 ms of PIT counts.
const CALIBRATION_MS: u32 = 10;

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: caller names a valid byte-wide port.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: caller names a valid byte-wide port.
    unsafe {
        core::arch::asm!("in al, dx", out("al") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}

/// Read the timestamp counter.
#[must_use]
pub fn rdtsc() -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY: RDTSC has no side effects and is available on every x86-64 CPU.
    unsafe {
        core::arch::asm!("rdtsc", out("eax") low, out("edx") high,
            options(nomem, nostack, preserves_flags));
    }
    (u64::from(high) << 32) | u64::from(low)
}

/// Measure the TSC frequency in hertz by timing a fixed PIT channel-2 one-shot.
///
/// # Safety
/// CPL0. Uses PIT channel 2 and port 0x61, which nothing else drives here.
unsafe fn calibrate_hz() -> u64 {
    let count = (PIT_INPUT_HZ * CALIBRATION_MS / 1000) as u16;

    unsafe {
        // Gate channel 2 on, speaker off (bit 1 clear).
        let prior = inb(PORT_61);
        outb(PORT_61, (prior & 0xfc) | 0x01);
        // Channel 2, lo/hi byte, mode 0 (interrupt on terminal count), binary.
        outb(PIT_MODE_PORT, 0xb0);
        outb(PIT_CH2_PORT, (count & 0xff) as u8);
        outb(PIT_CH2_PORT, (count >> 8) as u8);

        let start = rdtsc();
        // Wait for the channel-2 output (bit 5) to go high at terminal count.
        let mut budget = 100_000_000u32;
        while inb(PORT_61) & 0x20 == 0 {
            budget -= 1;
            if budget == 0 {
                return 0;
            }
            core::hint::spin_loop();
        }
        let end = rdtsc();

        let delta = end.wrapping_sub(start);
        // freq = ticks / (count / PIT_INPUT_HZ) = ticks * PIT_INPUT_HZ / count.
        delta.saturating_mul(u64::from(PIT_INPUT_HZ)) / u64::from(count)
    }
}

/// Prove the TSC is monotonic and derive a plausible frequency.
pub fn prove() {
    debug_write("AW_CLOCK_BEGIN\n");

    let t1 = rdtsc();
    let t2 = rdtsc();
    let t3 = rdtsc();
    if !(t1 < t2 && t2 < t3) {
        debug_write("AW_CLOCK_FAIL reason=not_monotonic\n");
        return;
    }
    debug_write("AW_CLOCK_MONOTONIC_OK\n");

    // SAFETY: CPL0 bring-up; PIT channel 2 is otherwise unused.
    let hz = unsafe { calibrate_hz() };
    // A very loose plausibility window: the point is a real, positive frequency
    // measured against the PIT, not an exact value (which TCG does not model).
    if !(1_000_000..=1_000_000_000_000).contains(&hz) {
        debug_write("AW_CLOCK_FAIL reason=implausible_hz\n");
        return;
    }
    debug_write("AW_CLOCK_CALIBRATED_OK khz=");
    debug_write_u64(hz / 1000);
    debug_write("\n");
    debug_write("AW_CLOCK_PROOF_OK\n");
}
