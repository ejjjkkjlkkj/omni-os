//! CMOS real-time-clock read (dossier section 10 "time", roadmap P0 "wall clock").
//!
//! The TSC clock proof gives a monotonic tick; this reads the actual wall-clock
//! date and time from the CMOS RTC over ports 0x70/0x71, the way an OS learns what
//! time it is at boot. It waits out any update-in-progress, reads the fields twice
//! and accepts only a stable reading, decodes BCD when the RTC is in BCD mode, and
//! sanity-checks the result. Read-only and safe on any machine, so it runs on the
//! normal boot path.

use crate::{debug_write, debug_write_u64};

const CMOS_INDEX: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

const RTC_SECONDS: u8 = 0x00;
const RTC_MINUTES: u8 = 0x02;
const RTC_HOURS: u8 = 0x04;
const RTC_DAY: u8 = 0x07;
const RTC_MONTH: u8 = 0x08;
const RTC_YEAR: u8 = 0x09;
const RTC_STATUS_A: u8 = 0x0a;
const RTC_STATUS_B: u8 = 0x0b;

const STATUS_A_UPDATE_IN_PROGRESS: u8 = 1 << 7;
const STATUS_B_BINARY: u8 = 1 << 2; // set: values are binary, clear: BCD
const STATUS_B_24_HOUR: u8 = 1 << 1; // set: 24-hour, clear: 12-hour
const HOUR_PM_FLAG: u8 = 0x80; // in 12-hour mode, high bit means PM

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

fn read_register(reg: u8) -> u8 {
    // Bit 7 of the index port controls NMI; preserve it clear (NMI enabled) as is
    // conventional, and select the register in the low bits.
    // SAFETY: 0x70/0x71 are the architected CMOS index/data ports.
    unsafe {
        outb(CMOS_INDEX, reg);
        inb(CMOS_DATA)
    }
}

fn update_in_progress() -> bool {
    read_register(RTC_STATUS_A) & STATUS_A_UPDATE_IN_PROGRESS != 0
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0x0f) + ((value >> 4) * 10)
}

/// A decoded wall-clock reading.
struct DateTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

fn read_once() -> DateTime {
    DateTime {
        year: u16::from(read_register(RTC_YEAR)),
        month: read_register(RTC_MONTH),
        day: read_register(RTC_DAY),
        hour: read_register(RTC_HOURS),
        minute: read_register(RTC_MINUTES),
        second: read_register(RTC_SECONDS),
    }
}

fn fields_equal(a: &DateTime, b: &DateTime) -> bool {
    a.second == b.second
        && a.minute == b.minute
        && a.hour == b.hour
        && a.day == b.day
        && a.month == b.month
        && a.year == b.year
}

/// Read the RTC and prove a plausible wall-clock date/time. Emits `AW_RTC_*`.
pub fn prove() {
    debug_write("AW_RTC_BEGIN\n");

    // Read repeatedly until two consecutive reads agree with no update in progress,
    // so a read can never straddle an RTC tick. Bounded so a stuck RTC cannot hang.
    let mut previous = read_once();
    let mut stable = None;
    for _ in 0..1000 {
        if update_in_progress() {
            continue;
        }
        let current = read_once();
        if fields_equal(&previous, &current) {
            stable = Some(current);
            break;
        }
        previous = current;
    }
    let Some(mut dt) = stable else {
        debug_write("AW_RTC_FAIL reason=unstable\n");
        return;
    };

    // Decode according to Status Register B: BCD vs binary, 12- vs 24-hour.
    let status_b = read_register(RTC_STATUS_B);
    let is_pm = dt.hour & HOUR_PM_FLAG != 0;
    if status_b & STATUS_B_BINARY == 0 {
        dt.second = bcd_to_binary(dt.second);
        dt.minute = bcd_to_binary(dt.minute);
        dt.day = bcd_to_binary(dt.day);
        dt.month = bcd_to_binary(dt.month);
        dt.year = u16::from(bcd_to_binary(dt.year as u8));
        dt.hour = bcd_to_binary(dt.hour & !HOUR_PM_FLAG);
    } else {
        dt.hour &= !HOUR_PM_FLAG;
    }
    if status_b & STATUS_B_24_HOUR == 0 {
        // 12-hour mode: 12am -> 0, and PM adds 12 (except 12pm which stays 12).
        if dt.hour == 12 {
            dt.hour = 0;
        }
        if is_pm {
            dt.hour += 12;
        }
    }
    // The year register holds only the last two digits; assume the 2000s.
    let full_year = 2000 + dt.year;

    debug_write("AW_RTC_DATE year=");
    debug_write_u64(u64::from(full_year));
    debug_write(" month=");
    debug_write_u64(u64::from(dt.month));
    debug_write(" day=");
    debug_write_u64(u64::from(dt.day));
    debug_write(" time=");
    debug_write_u64(u64::from(dt.hour));
    debug_write(":");
    debug_write_u64(u64::from(dt.minute));
    debug_write(":");
    debug_write_u64(u64::from(dt.second));
    debug_write("\n");

    // Sanity-check the reading: a real RTC gives a valid calendar date in a
    // plausible range. This catches a dead RTC returning 0x00 or 0xFF everywhere.
    let plausible = (2020..=2099).contains(&full_year)
        && (1..=12).contains(&dt.month)
        && (1..=31).contains(&dt.day)
        && dt.hour < 24
        && dt.minute < 60
        && dt.second < 60;
    if plausible {
        debug_write("AW_RTC_PROOF_OK\n");
    } else {
        debug_write("AW_RTC_FAIL reason=implausible\n");
    }
}
