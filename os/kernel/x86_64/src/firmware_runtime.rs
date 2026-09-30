//! UEFI runtime services from the kernel (UEFI 2.11 chapter 8) and the trial-boot health record.
//!
//! The loader hands over `EFI_RUNTIME_SERVICES` and, when the Memory Attributes Table splits the
//! runtime images into read-only code and non-executable data, the code ranges: `virtual_memory`
//! maps exactly those ranges read-only and executable and firmware code runs under the kernel's
//! own W^X page tables. Otherwise (no usable table: runtime code that is also writable), each
//! firmware call runs on the firmware's own page tables, still intact because the kernel only
//! reuses conventional memory: interrupts masked, CR3 switched for the duration of the call,
//! then the kernel's tables are back. Either way the kernel's tables never hold a page that is
//! both writable and executable. No virtual address map is set: the firmware keeps its
//! identity addresses.
//!
//! During a trial attempt the kernel's proofs mark the runtime-health checks they establish
//! ([`pass`]). At the end of bring-up, [`record`] writes the result as the `OmniHealth` variable
//! (`aw_bootstate::HealthRecord`, bound to the attempt's generation and boot-state sequence),
//! reads it back and compares. The loader decides the promotion on the next boot; the kernel
//! never marks itself known-good.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use aw_bootstate::{
    HEALTH_RECORD_BYTES, HEALTH_VARIABLE_ATTRIBUTES, HEALTH_VARIABLE_NAME, HEALTH_VENDOR_GUID,
    HealthRecord, KERNEL_HEALTH_CHECKS,
};
use aw_generation::RuntimeHealthCheck;
use aw_kernel_core::{BOOT_ATTEMPT_TRIAL, HANDOFF_FLAG_FIRMWARE_RUNTIME_PRESENT, KernelHandoff};

use crate::{debug_write, debug_write_u64};

/// `EFI_RUNTIME_SERVICES` signature "RUNTSERV".
const RUNTIME_SERVICES_SIGNATURE: u64 = 0x5652_4553_544e_5552;
/// Table header (24 bytes), then GetTime .. ConvertPointer (6 slots): GetVariable is slot 6.
const GET_VARIABLE_OFFSET: usize = 24 + 6 * 8;
const SET_VARIABLE_OFFSET: usize = 24 + 8 * 8;
const EFI_SUCCESS: usize = 0;

type GetVariable =
    unsafe extern "efiapi" fn(*const u16, *const [u8; 16], *mut u32, *mut usize, *mut u8) -> usize;
type SetVariable =
    unsafe extern "efiapi" fn(*const u16, *const [u8; 16], u32, usize, *const u8) -> usize;

static PASSED: AtomicU16 = AtomicU16::new(0);
/// How runtime services were reached at the last [`record`]: 0 unknown, 1 mapped, 2 firmware
/// tables, 3 firmware tables still active, 4 unavailable.
static MODE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The health checks passed so far (`RuntimeHealthCheck::bit` mask).
pub fn passed() -> u16 {
    PASSED.load(Ordering::SeqCst)
}

/// A spoken description of how the kernel reaches UEFI runtime services.
pub fn mode_label() -> &'static str {
    match MODE.load(Ordering::SeqCst) {
        1 => "code du micrologiciel en lecture et exécution seules",
        2 => "appels isolés sur les tables du micrologiciel",
        3 => "tables du micrologiciel encore actives",
        4 => "indisponibles",
        _ => "non évalués",
    }
}
static CODE_MAPPED: AtomicBool = AtomicBool::new(false);
/// The kernel's own page tables are active.
static KERNEL_MAP: AtomicBool = AtomicBool::new(false);
/// CR3 the firmware left active (its identity map), for `firmware_tables` calls.
static FIRMWARE_CR3: AtomicU64 = AtomicU64::new(0);
const CR4_SMEP: u64 = 1 << 20;
const CR4_SMAP: u64 = 1 << 21;

/// A kernel proof established `check`.
pub fn pass(check: RuntimeHealthCheck) {
    PASSED.fetch_or(check.bit(), Ordering::SeqCst);
}

/// The kernel's page tables are active. `code_mapped`: runtime code is mapped executable in them;
/// `firmware_cr3`: the firmware's page tables, used for calls otherwise.
pub fn set_kernel_map(firmware_cr3: u64, code_mapped: bool) {
    FIRMWARE_CR3.store(firmware_cr3, Ordering::SeqCst);
    CODE_MAPPED.store(code_mapped, Ordering::SeqCst);
    KERNEL_MAP.store(true, Ordering::SeqCst);
}

/// Runtime code ranges to map, when the handoff carries them.
pub fn code_ranges(handoff: &KernelHandoff, out: &mut [(u64, u64)]) -> usize {
    if handoff.flags & HANDOFF_FLAG_FIRMWARE_RUNTIME_PRESENT == 0 {
        return 0;
    }
    let mut count = 0;
    for range in handoff.firmware.code() {
        if count == out.len() {
            break;
        }
        out[count] = (range.start, range.end);
        count += 1;
    }
    count
}

fn name() -> [u16; 16] {
    let mut out = [0_u16; 16];
    for (slot, unit) in out.iter_mut().zip(HEALTH_VARIABLE_NAME.encode_utf16()) {
        *slot = unit;
    }
    out
}

/// Runs the firmware call `f` with interrupts masked (runtime code is not reentrant) and, in
/// `firmware_tables` mode, on the firmware's page tables (`switch_to` = its CR3).
fn firmware_call<T>(switch_to: Option<u64>, f: impl FnOnce() -> T) -> T {
    let flags: u64;
    // SAFETY: CPL0; save RFLAGS and mask interrupts around the firmware call.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem)) };
    let result = match switch_to {
        None => f(),
        Some(firmware_cr3) => {
            let (kernel_cr3, cr4): (u64, u64);
            // SAFETY: CPL0 with interrupts masked. The firmware tables identity-map all memory,
            // including this code and stack; SMEP/SMAP are lifted for the call because those
            // tables may mark pages user-accessible, and everything is restored before returning.
            unsafe {
                asm!("mov {}, cr3", out(reg) kernel_cr3, options(nomem, nostack));
                asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
                asm!("mov cr4, {}", in(reg) cr4 & !(CR4_SMEP | CR4_SMAP), options(nostack));
                asm!("mov cr3, {}", in(reg) firmware_cr3, options(nostack));
            }
            let result = f();
            // SAFETY: back to the kernel's W^X tables and protections.
            unsafe {
                asm!("mov cr3, {}", in(reg) kernel_cr3, options(nostack));
                asm!("mov cr4, {}", in(reg) cr4, options(nostack));
            }
            result
        }
    };
    if flags & (1 << 9) != 0 {
        // SAFETY: interrupts were enabled before; restore them.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    result
}

/// Reports the health checks and, during a trial attempt, records them for the loader.
pub fn record(handoff: &KernelHandoff) {
    let passed = PASSED.load(Ordering::SeqCst);
    debug_write("AW_HEALTH_CHECKS");
    for check in KERNEL_HEALTH_CHECKS {
        debug_write(" ");
        debug_write(check_name(check));
        debug_write(if passed & check.bit() != 0 {
            "=pass"
        } else {
            "=fail"
        });
    }
    debug_write("\n");

    if handoff.flags & HANDOFF_FLAG_FIRMWARE_RUNTIME_PRESENT == 0 {
        debug_write("AW_UEFI_RUNTIME_UNAVAILABLE reason=no_handoff\n");
        return;
    }
    // How firmware code is reached: its own mapping in the kernel's tables, the firmware's
    // tables for the duration of the call, or the firmware's tables still active.
    MODE.store(4, Ordering::SeqCst);
    let (switch_to, mode) = if !KERNEL_MAP.load(Ordering::SeqCst) {
        (None, "firmware_tables_active")
    } else if CODE_MAPPED.load(Ordering::SeqCst) {
        (None, "mapped")
    } else {
        match FIRMWARE_CR3.load(Ordering::SeqCst) {
            0 => {
                debug_write("AW_UEFI_RUNTIME_UNAVAILABLE reason=no_firmware_tables\n");
                return;
            }
            cr3 => (Some(cr3 & !0xfff), "firmware_tables"),
        }
    };
    let table = handoff.firmware.runtime_services as usize;
    // SAFETY: the loader validated this pointer before ExitBootServices; runtime-services data
    // stays identity-mapped (RW, NX) in the kernel's page tables.
    let signature = unsafe { core::ptr::read_volatile(table as *const u64) };
    if signature != RUNTIME_SERVICES_SIGNATURE {
        debug_write("AW_UEFI_RUNTIME_UNAVAILABLE reason=bad_signature\n");
        return;
    }
    MODE.store(
        match mode {
            "mapped" => 1,
            "firmware_tables" => 2,
            _ => 3,
        },
        Ordering::SeqCst,
    );
    debug_write("AW_UEFI_RUNTIME_READY mode=");
    debug_write(mode);
    debug_write(" code_ranges=");
    debug_write_u64(u64::from(handoff.firmware.code_range_count));
    debug_write("\n");

    let firmware = handoff.firmware;
    if firmware.boot_attempt != BOOT_ATTEMPT_TRIAL {
        debug_write("AW_HEALTH_NOT_RECORDED reason=not_a_trial_attempt\n");
        return;
    }
    let health = HealthRecord::new(firmware.trial_generation, firmware.trial_sequence, passed);
    let bytes = health.encode();
    let name = name();
    // SAFETY: slot addresses inside the validated runtime services table.
    let (get, set) = unsafe {
        (
            core::ptr::read_volatile((table + GET_VARIABLE_OFFSET) as *const GetVariable),
            core::ptr::read_volatile((table + SET_VARIABLE_OFFSET) as *const SetVariable),
        )
    };
    // SAFETY: SetVariable/GetVariable with NUL-terminated name, vendor GUID and our buffers; the
    // firmware runtime code and data are mapped (see module docs).
    let (written, read_back) = firmware_call(switch_to, || unsafe {
        let written = set(
            name.as_ptr(),
            &HEALTH_VENDOR_GUID,
            HEALTH_VARIABLE_ATTRIBUTES,
            bytes.len(),
            bytes.as_ptr(),
        );
        let mut back = [0_u8; HEALTH_RECORD_BYTES];
        let mut size = back.len();
        let mut attributes = 0_u32;
        let status = get(
            name.as_ptr(),
            &HEALTH_VENDOR_GUID,
            &mut attributes,
            &mut size,
            back.as_mut_ptr(),
        );
        (
            written,
            status == EFI_SUCCESS && size == back.len() && back == bytes,
        )
    });
    if written != EFI_SUCCESS || !read_back {
        debug_write("AW_HEALTH_RECORD_FAIL status=");
        debug_write_u64(written as u64);
        debug_write("\n");
        return;
    }
    debug_write("AW_HEALTH_RECORDED generation=");
    debug_write_u64(firmware.trial_generation);
    debug_write(" sequence=");
    debug_write_u64(firmware.trial_sequence);
    debug_write(" mask=");
    debug_write_u64(u64::from(health.passed_mask()));
    debug_write("\n");
}

const fn check_name(check: RuntimeHealthCheck) -> &'static str {
    match check {
        RuntimeHealthCheck::Kernel => "kernel",
        RuntimeHealthCheck::Storage => "storage",
        RuntimeHealthCheck::Input => "input",
        RuntimeHealthCheck::Audio => "audio",
        RuntimeHealthCheck::AccessibilityBroker => "accessibility",
        RuntimeHealthCheck::Speech => "speech",
        RuntimeHealthCheck::AccessibleRecovery => "recovery",
        RuntimeHealthCheck::Security => "security",
        RuntimeHealthCheck::Update => "update",
    }
}
