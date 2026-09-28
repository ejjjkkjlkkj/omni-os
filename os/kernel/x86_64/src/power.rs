//! ACPI power off and reset (roadmap Phase 3: "ACPI power off/reboot").
//!
//! [`init`] reads the FADT and the `\_S5` sleep package once at boot. Power off
//! then enters the S5 soft-off state: switch the chipset to ACPI mode if firmware
//! left it in legacy mode, and write SLP_TYPx | SLP_EN into the PM1a (and PM1b)
//! control registers. Reset uses the FADT reset register when the FADT declares
//! it, and otherwise the 8042 reset line with its triple-fault fallback
//! ([`crate::ps2_keyboard::reboot`]).

use crate::acpi::PowerControl;
use crate::debug_write;

const SLP_EN: u16 = 1 << 13;
const SCI_EN: u16 = 1;

// Written once by `init` on the bootstrap processor before anything reads it.
static mut CONTROL: Option<PowerControl> = None;

fn control() -> Option<PowerControl> {
    // SAFETY: written once during single-threaded boot, read-only afterwards.
    unsafe { *core::ptr::addr_of!(CONTROL) }
}

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: caller-validated I/O port write.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags))
    };
}
unsafe fn outw(port: u16, value: u16) {
    // SAFETY: caller-validated I/O port write.
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags))
    };
}
unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: caller-validated I/O port read.
    unsafe {
        core::arch::asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack, preserves_flags))
    };
    value
}
#[cfg(feature = "acpi-reset-test")]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: caller-validated I/O port read.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags))
    };
    value
}

fn describe(control: &PowerControl) {
    debug_write("AW_ACPI_POWER_CONTROL pm1a=");
    crate::debug_write_hex_u64(u64::from(control.fadt.pm1a_cnt));
    debug_write(" s5=");
    match control.s5 {
        Some((a, b)) => {
            crate::debug_write_u64(u64::from(a));
            debug_write("/");
            crate::debug_write_u64(u64::from(b));
        }
        None => debug_write("none"),
    }
    debug_write(" reset=");
    match control.fadt.reset {
        Some((gas, value)) => {
            crate::debug_write_hex_u64(gas.address);
            debug_write(":");
            crate::debug_write_hex_u64(u64::from(value));
        }
        None => debug_write("none"),
    }
    debug_write("\n");
}

/// Read the power controls from ACPI once.
///
/// # Safety
/// CPL0 on the bootstrap processor during boot, after the identity map is live,
/// with `rsdp` from the validated boot handoff.
pub unsafe fn init(rsdp: u64) {
    // SAFETY: caller's contract.
    match unsafe { crate::acpi::find_power_control(rsdp) } {
        Ok(found) => {
            describe(&found);
            // SAFETY: single-threaded boot; nothing reads CONTROL yet.
            unsafe { *core::ptr::addr_of_mut!(CONTROL) = Some(found) };
        }
        Err(error) => {
            debug_write("AW_ACPI_POWER_CONTROL_UNAVAILABLE reason=");
            debug_write(error.name());
            debug_write("\n");
        }
    }
}

/// Put the chipset in ACPI mode (SCI_EN set) if firmware has not already.
///
/// # Safety
/// CPL0; the ports come from a validated FADT.
unsafe fn enable_acpi_mode(control: &PowerControl) {
    let pm1a = control.fadt.pm1a_cnt as u16;
    // SAFETY: PM1a_CNT from the FADT.
    if unsafe { inw(pm1a) } & SCI_EN != 0
        || control.fadt.smi_cmd == 0
        || control.fadt.acpi_enable == 0
    {
        return;
    }
    // SAFETY: SMI_CMD/ACPI_ENABLE from the FADT: the documented handshake.
    unsafe { outb(control.fadt.smi_cmd as u16, control.fadt.acpi_enable) };
    for _ in 0..1_000_000 {
        // SAFETY: as above.
        if unsafe { inw(pm1a) } & SCI_EN != 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

fn halt_forever() -> ! {
    loop {
        // SAFETY: interrupts masked; nothing is left to run.
        unsafe { core::arch::asm!("cli; hlt", options(nomem, nostack)) };
    }
}

/// Enter S5 (soft off). Never returns: if the firmware ignores the request the
/// CPU halts with a marker rather than rebooting unexpectedly.
///
/// # Safety
/// CPL0, the very last action of the kernel.
pub unsafe fn power_off_machine() -> ! {
    let Some(control) = control() else {
        debug_write("AW_ACPI_POWEROFF_FAIL reason=no_fadt\n");
        halt_forever()
    };
    let Some((typa, typb)) = control.s5 else {
        debug_write("AW_ACPI_POWEROFF_FAIL reason=no_s5\n");
        halt_forever()
    };
    debug_write("AW_ACPI_POWEROFF_ISSUED\n");
    // SAFETY: CPL0; all ports come from the validated FADT.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
        enable_acpi_mode(&control);
        let pm1a = control.fadt.pm1a_cnt as u16;
        outw(
            pm1a,
            (inw(pm1a) & !(7 << 10)) | (u16::from(typa) << 10) | SLP_EN,
        );
        if control.fadt.pm1b_cnt != 0 {
            let pm1b = control.fadt.pm1b_cnt as u16;
            outw(
                pm1b,
                (inw(pm1b) & !(7 << 10)) | (u16::from(typb) << 10) | SLP_EN,
            );
        }
    }
    for _ in 0..10_000_000 {
        core::hint::spin_loop();
    }
    debug_write("AW_ACPI_POWEROFF_FAIL reason=still_running\n");
    halt_forever()
}

/// Reset the machine: the FADT reset register if declared, then the 8042 and
/// its triple-fault fallback.
///
/// # Safety
/// CPL0, the very last action of the kernel.
pub unsafe fn reset_machine() -> ! {
    // SAFETY: CPL0.
    unsafe { core::arch::asm!("cli", options(nomem, nostack)) };
    if let Some((gas, value)) = control().and_then(|c| c.fadt.reset) {
        match gas.space {
            1 if gas.address <= u64::from(u16::MAX) => {
                debug_write("AW_POWER_RESET_PATH fadt_io\n");
                // SAFETY: I/O reset register declared by the FADT.
                unsafe { outb(gas.address as u16, value) };
            }
            0 if gas.address < (crate::virtual_memory::IDENTITY_GIB << 30) => {
                debug_write("AW_POWER_RESET_PATH fadt_mmio\n");
                // SAFETY: identity-mapped reset register declared by the FADT.
                unsafe { (gas.address as usize as *mut u8).write_volatile(value) };
            }
            _ => {}
        }
        for _ in 0..10_000_000 {
            core::hint::spin_loop();
        }
    }
    debug_write("AW_POWER_RESET_PATH i8042\n");
    // SAFETY: CPL0; 8042 reset line, then a forced triple fault.
    unsafe { crate::ps2_keyboard::reboot() }
}

// ---- Proofs -------------------------------------------------------------------

/// Power-off proof: the harness requires QEMU to exit on its own right after
/// the marker, which only a real S5 transition does.
///
/// # Safety
/// CPL0, the last thing the kernel does.
#[cfg(feature = "acpi-poweroff-test")]
pub unsafe fn prove_power_off() {
    // SAFETY: caller's contract.
    unsafe { power_off_machine() }
}

/// CMOS byte remembering, across a reset, that the reset proof already fired
/// (QEMU and PCs keep CMOS RAM across a warm reset).
#[cfg(feature = "acpi-reset-test")]
const CMOS_PROOF_INDEX: u8 = 0x5d;
#[cfg(feature = "acpi-reset-test")]
const CMOS_PROOF_MAGIC: u8 = 0xa5;

#[cfg(feature = "acpi-reset-test")]
fn cmos_read(index: u8) -> u8 {
    // SAFETY: CMOS index/data ports; NMI stays disabled (bit 7).
    unsafe {
        outb(0x70, 0x80 | index);
        inb(0x71)
    }
}
#[cfg(feature = "acpi-reset-test")]
fn cmos_write(index: u8, value: u8) {
    // SAFETY: as above.
    unsafe {
        outb(0x70, 0x80 | index);
        outb(0x71, value);
    }
}

/// Reset proof, over two boots: the first records a CMOS flag and resets; the
/// second finds the flag, clears it and reports - so the second boot exists
/// only because the reset happened.
///
/// # Safety
/// CPL0, at the end of boot.
#[cfg(feature = "acpi-reset-test")]
pub unsafe fn prove_reset() {
    if cmos_read(CMOS_PROOF_INDEX) == CMOS_PROOF_MAGIC {
        cmos_write(CMOS_PROOF_INDEX, 0);
        debug_write("AW_ACPI_RESET_PROOF_OK second_boot=1\n");
        return;
    }
    cmos_write(CMOS_PROOF_INDEX, CMOS_PROOF_MAGIC);
    debug_write("AW_ACPI_RESET_ISSUED\n");
    // SAFETY: last action of this boot.
    unsafe { reset_machine() }
}
