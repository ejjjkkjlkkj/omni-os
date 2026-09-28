//! COM1 (16550 UART) mirror for the firmware stage.
//!
//! The UEFI boot application streams its `AW_UEFI_*` markers to the 0xE9 debug
//! port through the `uefi` crate's logger. That port is a QEMU/Bochs convenience:
//! VMware Workstation and real hardware do not have it, so on those the
//! firmware-stage accessibility - the spoken screen reader and its menu - would
//! run without leaving any machine-checkable trace. A 16550 UART is a real,
//! ubiquitous device VMware exposes as COM1 with a file backend, and physical
//! machines expose over a header or USB adapter.
//!
//! This brings COM1 up at 115200 8N1, proves it present with an internal loopback
//! test (a transmitted byte must reappear on the receive side), and then mirrors
//! the accessibility markers onto the real line through the [`aw_mark!`] macro. It
//! is deliberately independent of the `uefi` logger: it uses only the architected
//! COM1 I/O ports, so it needs no boot service and keeps working across the whole
//! firmware stage. When COM1 is absent (the QEMU proofs run with no serial
//! backend) the loopback readback does not match, the mirror stays off, and the
//! 0xE9 markers the QEMU suite asserts are unaffected.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

const COM1: u16 = 0x3f8;

// Register offsets from the port base (DLAB selects the divisor latches).
const REG_DATA: u16 = 0; // THR (write) / RBR (read), or DLL when DLAB=1
const REG_IER: u16 = 1; // interrupt enable, or DLM when DLAB=1
const REG_FCR: u16 = 2; // FIFO control (write)
const REG_LCR: u16 = 3; // line control
const REG_MCR: u16 = 4; // modem control
const REG_LSR: u16 = 5; // line status

const LSR_DATA_READY: u8 = 1 << 0;
const LSR_THR_EMPTY: u8 = 1 << 5;

const MCR_DTR_RTS_OUT2: u8 = 0x0b; // DTR | RTS | OUT2, normal operation
const MCR_LOOPBACK: u8 = 0x1e; // loopback | OUT2 | OUT1 | RTS

const LOOPBACK_BYTE: u8 = 0xae;

/// Set once COM1 is proved present. Until then [`mirror`] is a no-op, so a machine
/// with no COM1 is unaffected and the 0xE9 markers stand alone.
static PRESENT: AtomicBool = AtomicBool::new(false);

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: the caller names a valid byte-wide port; a UEFI application runs at
    // CPL0 with port I/O permitted.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: the caller names a valid byte-wide port.
    unsafe {
        core::arch::asm!("in al, dx", out("al") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}

/// Program COM1 for 115200 8N1 with the FIFO enabled.
///
/// # Safety
/// CPL0. Uses the architected COM1 I/O ports.
unsafe fn configure() {
    unsafe {
        outb(COM1 + REG_IER, 0x00); // no interrupts: this console is polled
        outb(COM1 + REG_LCR, 0x80); // DLAB on
        outb(COM1 + REG_DATA, 0x01); // divisor low = 1 -> 115200 baud
        outb(COM1 + REG_IER, 0x00); // divisor high = 0
        outb(COM1 + REG_LCR, 0x03); // DLAB off, 8 bits, no parity, 1 stop
        outb(COM1 + REG_FCR, 0xc7); // enable + clear FIFOs, 14-byte trigger
        outb(COM1 + REG_MCR, MCR_DTR_RTS_OUT2);
    }
}

/// Prove the UART responds: in loopback, a transmitted byte must return on the
/// receive side. Returns `false` if COM1 is absent (readback does not match).
///
/// # Safety
/// CPL0, after [`configure`].
unsafe fn loopback_ok() -> bool {
    unsafe {
        outb(COM1 + REG_MCR, MCR_LOOPBACK);
        outb(COM1 + REG_DATA, LOOPBACK_BYTE);

        let mut budget = 100_000u32;
        while inb(COM1 + REG_LSR) & LSR_DATA_READY == 0 {
            budget -= 1;
            if budget == 0 {
                outb(COM1 + REG_MCR, MCR_DTR_RTS_OUT2);
                return false;
            }
            core::hint::spin_loop();
        }
        let echoed = inb(COM1 + REG_DATA);
        outb(COM1 + REG_MCR, MCR_DTR_RTS_OUT2);
        echoed == LOOPBACK_BYTE
    }
}

/// Write one byte to COM1, waiting for the transmit register to drain first.
///
/// # Safety
/// CPL0, after a successful [`configure`].
unsafe fn write_byte(byte: u8) {
    unsafe {
        let mut budget = 100_000u32;
        while inb(COM1 + REG_LSR) & LSR_THR_EMPTY == 0 {
            budget -= 1;
            if budget == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        outb(COM1 + REG_DATA, byte);
    }
}

/// A `core::fmt::Write` sink onto COM1 that turns `\n` into CR+LF, so a captured
/// serial log lands on line boundaries like the debug console does.
struct Port;

impl Write for Port {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if byte == b'\n' {
                // SAFETY: COM1 was configured and proved before PRESENT was set,
                // which is the only path that reaches a live `Port`.
                unsafe { write_byte(b'\r') };
            }
            // SAFETY: as above.
            unsafe { write_byte(byte) };
        }
        Ok(())
    }
}

/// Bring COM1 up and prove it. On success the mirror is armed and a banner is put
/// on the real line so a captured log is self-identifying; on failure the mirror
/// stays off. Safe to call once, early in the firmware stage.
pub fn init() -> bool {
    // SAFETY: CPL0 bring-up touching only COM1's architected ports.
    let present = unsafe {
        configure();
        loopback_ok()
    };
    PRESENT.store(present, Ordering::Relaxed);
    if present {
        let _ = writeln!(Port, "AW_UEFI_SERIAL_OK accessible-windows firmware-stage");
    }
    present
}

/// Mirror one already-formatted marker line onto COM1, followed by CR+LF. A no-op
/// until [`init`] proves the port present. Never allocates.
pub fn mirror(args: fmt::Arguments) {
    if PRESENT.load(Ordering::Relaxed) {
        let mut port = Port;
        let _ = port.write_fmt(args);
        let _ = port.write_str("\r\n");
    }
}
