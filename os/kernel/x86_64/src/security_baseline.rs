#![allow(dead_code)]

use core::arch::{
    asm,
    x86_64::{__cpuid, __cpuid_count},
};

const IA32_EFER_MSR: u32 = 0xC000_0080;

const CR0_WP: u64 = 1 << 16;
const EFER_NXE: u64 = 1 << 11;

const CR4_UMIP: u64 = 1 << 11;
const CR4_SMEP: u64 = 1 << 20;
const CR4_SMAP: u64 = 1 << 21;

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuSecurityCapabilities {
    pub nx: bool,
    pub smep: bool,
    pub smap: bool,
    pub umip: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeSecurityState {
    pub cr0_write_protect: bool,
    pub efer_nx_enable: bool,
    pub cr4_smep: bool,
    pub cr4_smap: bool,
    pub cr4_umip: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub enum RuntimeSecurityGap {
    WriteProtectDisabled,
    NxSupportedButDisabled,
    SmepSupportedButDisabled,
    SmapSupportedButDisabled,
    UmipSupportedButDisabled,
}

impl RuntimeSecurityGap {
    pub const fn name(self) -> &'static str {
        match self {
            Self::WriteProtectDisabled => "cr0-write-protect-disabled",
            Self::NxSupportedButDisabled => "nx-supported-but-disabled",
            Self::SmepSupportedButDisabled => "smep-supported-but-disabled",
            Self::SmapSupportedButDisabled => "smap-supported-but-disabled",
            Self::UmipSupportedButDisabled => "umip-supported-but-disabled",
        }
    }
}

pub fn capabilities() -> CpuSecurityCapabilities {
    let mut caps = CpuSecurityCapabilities::default();

    let max_basic = __cpuid(0).eax;
    if max_basic >= 7 {
        let leaf7 = __cpuid_count(7, 0);
        caps.smep = (leaf7.ebx & (1 << 7)) != 0;
        caps.smap = (leaf7.ebx & (1 << 20)) != 0;
        caps.umip = (leaf7.ecx & (1 << 2)) != 0;
    }

    let max_extended = __cpuid(0x8000_0000).eax;
    if max_extended >= 0x8000_0001 {
        let ext1 = __cpuid(0x8000_0001);
        caps.nx = (ext1.edx & (1 << 20)) != 0;
    }

    caps
}

/// # Safety
/// CPL0 only.
unsafe fn read_cr0() -> u64 {
    let value: u64;
    unsafe {
        asm!(
            "mov {}, cr0",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// # Safety
/// CPL0 only.
unsafe fn read_cr4() -> u64 {
    let value: u64;
    unsafe {
        asm!(
            "mov {}, cr4",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// # Safety
/// CPL0 only. MSR must exist.
unsafe fn rdmsr(msr: u32) -> u64 {
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
/// CPL0 only.
unsafe fn write_cr0(value: u64) {
    unsafe {
        asm!("mov cr0, {}", in(reg) value, options(nostack, preserves_flags));
    }
}

/// # Safety
/// CPL0 only.
unsafe fn write_cr4(value: u64) {
    unsafe {
        asm!("mov cr4, {}", in(reg) value, options(nostack, preserves_flags));
    }
}

/// # Safety
/// CPL0 only. MSR must exist.
unsafe fn wrmsr(msr: u32, value: u64) {
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

/// Turn on every CPU protection the hardware reports, then re-read the state.
///
/// Reporting a gap is not the same as closing it: until these bits are set,
/// read-only page-table entries do not stop supervisor writes (CR0.WP), NX bits
/// are ignored (EFER.NXE), and supervisor code may execute or read user pages
/// (CR4.SMEP/SMAP). The kernel therefore enables what the CPU supports before
/// it claims any memory protection, and the caller proves the result with real
/// faults rather than trusting this function.
///
/// Unsupported features are skipped rather than forced, so this is safe on
/// older CPUs; a feature that is supported but refuses to latch shows up as a
/// remaining gap in [`first_security_gap`].
///
/// # Safety
/// CPL0 only, during controlled bring-up, after the kernel owns its page
/// tables. Enabling SMEP/SMAP requires that no supervisor code is executing
/// from, or dereferencing, user-accessible pages.
pub unsafe fn enforce_baseline() -> RuntimeSecurityState {
    let caps = capabilities();

    // SAFETY: CPL0. Setting CR0.WP only makes read-only mappings authoritative
    // for supervisor writes; it never unmaps anything.
    unsafe {
        let cr0 = read_cr0();
        if cr0 & CR0_WP == 0 {
            write_cr0(cr0 | CR0_WP);
        }
    }

    if caps.nx {
        // SAFETY: CPL0, and EFER exists on every long-mode CPU.
        unsafe {
            let efer = rdmsr(IA32_EFER_MSR);
            if efer & EFER_NXE == 0 {
                wrmsr(IA32_EFER_MSR, efer | EFER_NXE);
            }
        }
    }

    let mut cr4_wanted = 0;
    if caps.smep {
        cr4_wanted |= CR4_SMEP;
    }
    if caps.smap {
        cr4_wanted |= CR4_SMAP;
    }
    if caps.umip {
        cr4_wanted |= CR4_UMIP;
    }

    if cr4_wanted != 0 {
        // SAFETY: CPL0; only sets supervisor-protection bits, and no
        // user-accessible page is mapped at this point in bring-up.
        unsafe {
            let cr4 = read_cr4();
            if cr4 & cr4_wanted != cr4_wanted {
                write_cr4(cr4 | cr4_wanted);
            }
        }
    }

    // SAFETY: CPL0; read-only.
    unsafe { runtime_state() }
}

/// Reads security state without changing hardware configuration.
///
/// # Safety
/// CPL0 only.
pub unsafe fn runtime_state() -> RuntimeSecurityState {
    let cr0 = unsafe { read_cr0() };
    let cr4 = unsafe { read_cr4() };
    let efer = unsafe { rdmsr(IA32_EFER_MSR) };

    RuntimeSecurityState {
        cr0_write_protect: (cr0 & CR0_WP) != 0,
        efer_nx_enable: (efer & EFER_NXE) != 0,
        cr4_smep: (cr4 & CR4_SMEP) != 0,
        cr4_smap: (cr4 & CR4_SMAP) != 0,
        cr4_umip: (cr4 & CR4_UMIP) != 0,
    }
}

/// Strict post-memory-init verification. It does not enable bits.
///
/// SMEP/SMAP/UMIP are only required here when the CPU reports support.
/// If the kernel intentionally enables them later in boot, call this function
/// only after the relevant paging/user-memory policy is initialized.
///
/// # Safety
/// CPL0 only.
pub unsafe fn first_security_gap() -> Option<RuntimeSecurityGap> {
    let caps = capabilities();
    let state = unsafe { runtime_state() };

    if !state.cr0_write_protect {
        return Some(RuntimeSecurityGap::WriteProtectDisabled);
    }

    if caps.nx && !state.efer_nx_enable {
        return Some(RuntimeSecurityGap::NxSupportedButDisabled);
    }

    if caps.smep && !state.cr4_smep {
        return Some(RuntimeSecurityGap::SmepSupportedButDisabled);
    }

    if caps.smap && !state.cr4_smap {
        return Some(RuntimeSecurityGap::SmapSupportedButDisabled);
    }

    if caps.umip && !state.cr4_umip {
        return Some(RuntimeSecurityGap::UmipSupportedButDisabled);
    }

    None
}
