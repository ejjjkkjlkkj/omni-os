//! 16550 UART serial console on COM1 (dossier sections 4 and 24).
//!
//! The kernel already streams markers to the 0xE9 debug port, but that port is a
//! QEMU/Bochs convenience with no meaning on real hardware. A 16550 UART is a
//! real, ubiquitous device, and the dossier wants a serial console available for
//! early diagnosis - including on physical machines and in CI log analysis.
//!
//! This brings COM1 up at 115200 8N1, proves the device with an internal
//! loopback test (a byte written to the transmit register must reappear in the
//! receive register), then emits a banner on the real line. When COM1 is not
//! present (the emulator started with no serial backend) the loopback readback
//! does not match and the console is reported unavailable rather than assumed.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::debug_write;

const COM1: u16 = 0x3f8;

/// Set once COM1 is proved present, after which [`mirror_byte`] echoes every debug
/// marker onto the real line. Off by default, so a machine with no COM1 (the QEMU
/// proofs run `-serial none`) is unaffected.
static MIRROR: AtomicBool = AtomicBool::new(false);

/// Echo one debug byte onto COM1 if the console has been proved present. Called
/// from `debug_write`, so it must never call back into `debug_write`.
pub(crate) fn mirror_byte(byte: u8) {
    if MIRROR.load(Ordering::Relaxed) {
        // SAFETY: COM1 was configured and proved before the flag was set.
        unsafe { write_byte(byte) };
    }
}

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

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: the caller names a valid byte-wide port.
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
/// CPL0, after [`configure`].
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

/// Emit a string on the real serial line.
///
/// # Safety
/// CPL0, after a successful [`configure`]/[`loopback_ok`].
unsafe fn write_str(text: &str) {
    for byte in text.bytes() {
        // SAFETY: writing one byte at a time to a configured COM1.
        unsafe { write_byte(byte) };
    }
}

/// Bring up COM1 and prove it, then emit a banner on the line.
pub fn prove() {
    debug_write("AW_SERIAL_BEGIN\n");

    // SAFETY: CPL0 bring-up; only COM1's architected ports are touched.
    let present = unsafe {
        configure();
        loopback_ok()
    };
    if !present {
        debug_write("AW_SERIAL_UNAVAILABLE reason=no_uart\n");
        return;
    }
    debug_write("AW_SERIAL_LOOPBACK_OK byte=0xae\n");

    // COM1 is proved present: from here, mirror every debug marker onto the real
    // line so boots on hardware/hypervisors without a 0xE9 port are diagnosable.
    MIRROR.store(true, Ordering::Relaxed);

    // SAFETY: COM1 is configured and confirmed present.
    unsafe { write_str("AW-SERIAL-CONSOLE-OK accessible-windows\r\n") };
    debug_write("AW_SERIAL_PROOF_OK\n");
}
