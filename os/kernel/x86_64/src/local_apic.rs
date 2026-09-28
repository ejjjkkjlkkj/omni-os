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
