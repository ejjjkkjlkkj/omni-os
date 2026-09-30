#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
use core::arch::asm;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::__cpuid;

pub const IA32_APIC_BASE_MSR: u32 = 0x1B;
pub const IA32_APIC_BASE_BSP: u64 = 1 << 8;
pub const IA32_APIC_BASE_X2APIC_ENABLE: u64 = 1 << 10;
pub const IA32_APIC_BASE_GLOBAL_ENABLE: u64 = 1 << 11;
pub const IA32_APIC_BASE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

pub const X2APIC_MSR_BASE: u32 = 0x800;
pub const X2APIC_ID_MSR: u32 = 0x802;
pub const X2APIC_EOI_MSR: u32 = 0x80B;
pub const X2APIC_SIVR_MSR: u32 = 0x80F;
pub const X2APIC_LVT_TIMER_MSR: u32 = 0x832;
pub const X2APIC_TIMER_INITIAL_COUNT_MSR: u32 = 0x838;
pub const X2APIC_TIMER_CURRENT_COUNT_MSR: u32 = 0x839;
pub const X2APIC_TIMER_DIVIDE_MSR: u32 = 0x83E;

#[derive(Clone, Copy, Debug, Default)]
pub struct ApicCapabilities {
    pub local_apic: bool,
    pub x2apic: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ApicBase {
    pub raw: u64,
    pub physical_base: u64,
    pub enabled: bool,
    pub x2apic_enabled: bool,
    pub bootstrap_processor: bool,
}

#[cfg(target_arch = "x86_64")]
pub fn cpuid_capabilities() -> ApicCapabilities {
    let leaf1 = __cpuid(1);
    ApicCapabilities {
        local_apic: (leaf1.edx & (1 << 9)) != 0,
        x2apic: (leaf1.ecx & (1 << 21)) != 0,
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn cpuid_capabilities() -> ApicCapabilities {
    ApicCapabilities::default()
}

/// # Safety
/// RDMSR is privileged. Call only at CPL0 after confirming the MSR is valid.
#[cfg(target_arch = "x86_64")]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// # Safety
/// WRMSR is privileged. Call only at CPL0 after validating the target MSR/value.
#[cfg(target_arch = "x86_64")]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[cfg(target_arch = "x86_64")]
pub unsafe fn read_apic_base() -> ApicBase {
    let raw = unsafe { rdmsr(IA32_APIC_BASE_MSR) };
    ApicBase {
        raw,
        physical_base: raw & IA32_APIC_BASE_ADDR_MASK,
        enabled: (raw & IA32_APIC_BASE_GLOBAL_ENABLE) != 0,
        x2apic_enabled: (raw & IA32_APIC_BASE_X2APIC_ENABLE) != 0,
        bootstrap_processor: (raw & IA32_APIC_BASE_BSP) != 0,
    }
}

/// Enables the architectural Local APIC global-enable bit only.
/// This does not switch to x2APIC mode and does not touch MMIO mappings.
///
/// # Safety
/// Must execute at CPL0 during controlled CPU bring-up.
#[cfg(target_arch = "x86_64")]
pub unsafe fn enable_local_apic_global() -> ApicBase {
    let current = unsafe { rdmsr(IA32_APIC_BASE_MSR) };
    let new_value = current | IA32_APIC_BASE_GLOBAL_ENABLE;
    unsafe { wrmsr(IA32_APIC_BASE_MSR, new_value) };
    unsafe { read_apic_base() }
}

/// Switches xAPIC -> x2APIC only when CPUID reports support.
/// Keep this explicit: the V20 script does not call it automatically.
///
/// # Safety
/// Must execute at CPL0 before x2APIC MSRs are used, under kernel-controlled
/// interrupt/CPU initialization ordering.
#[cfg(target_arch = "x86_64")]
pub unsafe fn enable_x2apic() -> Result<ApicBase, &'static str> {
    let caps = cpuid_capabilities();
    if !caps.local_apic {
        return Err("Local APIC not reported by CPUID");
    }
    if !caps.x2apic {
        return Err("x2APIC not reported by CPUID");
    }

    let current = unsafe { rdmsr(IA32_APIC_BASE_MSR) };
    let enabled = current | IA32_APIC_BASE_GLOBAL_ENABLE | IA32_APIC_BASE_X2APIC_ENABLE;
    unsafe { wrmsr(IA32_APIC_BASE_MSR, enabled) };
    Ok(unsafe { read_apic_base() })
}

#[cfg(target_arch = "x86_64")]
pub unsafe fn x2apic_write(msr: u32, value: u32) {
    unsafe { wrmsr(msr, value as u64) };
}

#[cfg(target_arch = "x86_64")]
pub unsafe fn x2apic_read(msr: u32) -> u32 {
    unsafe { rdmsr(msr) as u32 }
}

#[cfg(target_arch = "x86_64")]
pub unsafe fn x2apic_eoi() {
    unsafe { wrmsr(X2APIC_EOI_MSR, 0) };
}

// ---- Either mode: x2APIC (MSRs) or xAPIC (MMIO) ------------------------------------------

/// xAPIC register offsets (the x2APIC MSR is 0x800 + offset / 16).
pub const LAPIC_ID: u32 = 0x20;
pub const LAPIC_EOI: u32 = 0xB0;
pub const LAPIC_SVR: u32 = 0xF0;
const SVR_APIC_SOFTWARE_ENABLE: u32 = 1 << 8;
const SVR_SPURIOUS_VECTOR: u32 = 0xFF;

/// MMIO base while the local APIC runs in xAPIC mode; 0 means x2APIC (MSRs).
static XAPIC_MMIO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Read a local APIC register by its xAPIC offset, in the mode selected by [`use_available_mode`].
///
/// # Safety
/// CPL0; the local APIC is enabled and, in xAPIC mode, its page is identity-mapped.
#[cfg(target_arch = "x86_64")]
pub unsafe fn lapic_read(offset: u32) -> u32 {
    match XAPIC_MMIO.load(core::sync::atomic::Ordering::Relaxed) {
        0 => unsafe { rdmsr(X2APIC_MSR_BASE + (offset >> 4)) as u32 },
        base => unsafe { core::ptr::read_volatile((base + u64::from(offset)) as *const u32) },
    }
}

/// Write a local APIC register by its xAPIC offset.
///
/// # Safety
/// As [`lapic_read`]; the value must be valid for the register.
#[cfg(target_arch = "x86_64")]
pub unsafe fn lapic_write(offset: u32, value: u32) {
    match XAPIC_MMIO.load(core::sync::atomic::Ordering::Relaxed) {
        0 => unsafe { wrmsr(X2APIC_MSR_BASE + (offset >> 4), u64::from(value)) },
        base => unsafe { core::ptr::write_volatile((base + u64::from(offset)) as *mut u32, value) },
    }
}

/// This CPU's local APIC ID (full 32 bits in x2APIC mode, bits 24..31 in xAPIC mode).
///
/// # Safety
/// As [`lapic_read`].
#[cfg(target_arch = "x86_64")]
pub unsafe fn lapic_id() -> u32 {
    let raw = unsafe { lapic_read(LAPIC_ID) };
    if XAPIC_MMIO.load(core::sync::atomic::Ordering::Relaxed) == 0 {
        raw
    } else {
        raw >> 24
    }
}

/// End of interrupt, in either mode.
///
/// # Safety
/// CPL0, from an interrupt handler of a vector delivered by this local APIC.
#[cfg(target_arch = "x86_64")]
pub unsafe fn lapic_eoi() {
    unsafe { lapic_write(LAPIC_EOI, 0) };
}

/// Select the local APIC access mode the CPU is really in: x2APIC when it is enabled, otherwise
/// xAPIC through its MMIO page (CPUs and emulators without x2APIC), software-enabled if the
/// firmware left it off. Returns the mode name.
///
/// # Safety
/// CPL0 during single-core bring-up; the APIC MMIO page lies in the identity-mapped window.
#[cfg(target_arch = "x86_64")]
pub unsafe fn use_available_mode() -> Result<&'static str, &'static str> {
    let base = unsafe { read_apic_base() };
    if !base.enabled {
        return Err("local_apic_disabled");
    }
    if base.x2apic_enabled {
        XAPIC_MMIO.store(0, core::sync::atomic::Ordering::Relaxed);
        return Ok("x2apic");
    }
    if base.physical_base == 0 || base.physical_base >= 1 << 32 {
        return Err("xapic_base_outside_identity_window");
    }
    XAPIC_MMIO.store(base.physical_base, core::sync::atomic::Ordering::Relaxed);
    let svr = unsafe { lapic_read(LAPIC_SVR) };
    if svr & SVR_APIC_SOFTWARE_ENABLE == 0 {
        unsafe {
            lapic_write(
                LAPIC_SVR,
                svr | SVR_APIC_SOFTWARE_ENABLE | SVR_SPURIOUS_VECTOR,
            )
        };
    }
    Ok("xapic")
}
