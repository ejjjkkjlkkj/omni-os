//! Legacy 8259 PIC remap-and-mask.
//!
//! UEFI firmware is not guaranteed to leave the legacy PIC masked or remapped.
//! An unmasked, unremapped IRQ0 delivers on vector 8 -- indistinguishable from
//! a real double fault. This must run before the first `sti` after boot.
#![allow(dead_code)]

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xa0;
const PIC2_DATA: u16 = 0xa1;
const PIC_UNUSED_IO_DELAY_PORT: u16 = 0x80;

const ICW1_INIT_ICW4: u8 = 0x11;
const ICW4_8086_MODE: u8 = 0x01;
const PIC1_VECTOR_BASE: u8 = 0x20;
const PIC2_VECTOR_BASE: u8 = 0x28;
const PIC1_CASCADE_IRQ2: u8 = 0x04;
const PIC2_CASCADE_IDENTITY: u8 = 0x02;
const MASK_ALL_LINES: u8 = 0xff;

/// # Safety
/// The caller ensures port 0x80 accepts byte writes, which holds on every
/// real and emulated x86 platform this kernel targets.
unsafe fn io_wait() {
    // SAFETY: port 0x80 is the conventional POST-diagnostic port used purely
    // to burn a bus cycle; nothing reads the value written here.
    unsafe { crate::outb(PIC_UNUSED_IO_DELAY_PORT, 0) };
}

/// Remap both legacy PICs off the CPU exception vector range and mask every
/// IRQ line so no legacy interrupt can be delivered.
///
/// # Safety
/// CPL0 only. Must run before the first `sti` after boot.
pub(crate) unsafe fn remap_and_mask_all() {
    unsafe {
        crate::outb(PIC1_COMMAND, ICW1_INIT_ICW4);
        io_wait();
        crate::outb(PIC2_COMMAND, ICW1_INIT_ICW4);
        io_wait();

        crate::outb(PIC1_DATA, PIC1_VECTOR_BASE);
        io_wait();
        crate::outb(PIC2_DATA, PIC2_VECTOR_BASE);
        io_wait();

        crate::outb(PIC1_DATA, PIC1_CASCADE_IRQ2);
        io_wait();
        crate::outb(PIC2_DATA, PIC2_CASCADE_IDENTITY);
        io_wait();

        crate::outb(PIC1_DATA, ICW4_8086_MODE);
        io_wait();
        crate::outb(PIC2_DATA, ICW4_8086_MODE);
        io_wait();

        crate::outb(PIC1_DATA, MASK_ALL_LINES);
        io_wait();
        crate::outb(PIC2_DATA, MASK_ALL_LINES);
        io_wait();
    }
}
