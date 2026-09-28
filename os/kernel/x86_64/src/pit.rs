//! The legacy 8254 programmable interval timer.
//!
//! This kernel does not want the PIT as a timekeeping source - the local APIC
//! timer already covers that. It wants it as the one interrupt source that is
//! present on every x86 platform, needs no driver, and is wired to a *pin*
//! rather than to the local APIC. That makes it the honest way to prove I/O
//! APIC routing: the interrupt has to leave a device, cross the I/O APIC, and
//! arrive on the vector the redirection entry names.

/// Channel 0's counter port. Channel 0's output is ISA IRQ 0.
const PIT_CHANNEL0_DATA: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

/// Channel 0 (bits 7:6 = 00), lobyte/hibyte access (bits 5:4 = 11), mode 2
/// rate generator (bits 3:1 = 010), binary rather than BCD counting (bit 0).
const PIT_CHANNEL0_MODE2_BINARY: u8 = (0b11 << 4) | (0b010 << 1);

/// Input frequency of the 8254 on every PC-compatible platform.
pub const PIT_INPUT_HZ: u32 = 1_193_182;

/// Divisor producing approximately `hz` interrupts per second.
///
/// A divisor of 0 means 65536 on this hardware, which is the slowest rate, so
/// requesting an unreachably low frequency saturates instead of wrapping to the
/// fastest one.
#[must_use]
pub const fn divisor_for_hz(hz: u32) -> u16 {
    if hz == 0 {
        return 0;
    }
    let divisor = PIT_INPUT_HZ / hz;
    if divisor == 0 {
        1
    } else if divisor > u16::MAX as u32 {
        0
    } else {
        divisor as u16
    }
}

/// Program channel 0 as a rate generator with the given divisor.
///
/// This starts the counter immediately; whether anything is delivered depends
/// entirely on the interrupt controller the line reaches.
///
/// # Safety
///
/// CPL0 only. The caller must have routed or masked ISA IRQ 0 first, so that
/// starting the counter cannot deliver through a controller that is not ready.
pub unsafe fn program_rate_generator(divisor: u16) {
    // SAFETY: the 8254 ports are fixed, byte-wide and present on every
    // supported platform.
    unsafe {
        crate::outb(PIT_COMMAND, PIT_CHANNEL0_MODE2_BINARY);
        crate::outb(PIT_CHANNEL0_DATA, divisor as u8);
        crate::outb(PIT_CHANNEL0_DATA, (divisor >> 8) as u8);
    }
}
