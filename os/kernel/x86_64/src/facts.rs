//! Facts about the running system, recorded by the drivers and proofs that establish them during
//! single-core bring-up, and read afterwards by the administration session. Every value is what
//! the kernel measured on this machine; nothing here is a default dressed up as a finding.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

/// A short ASCII string written once during bring-up.
pub struct Text<const N: usize> {
    bytes: [AtomicU8; N],
    len: AtomicU64,
}

impl<const N: usize> Text<N> {
    #[allow(clippy::declare_interior_mutable_const)]
    const EMPTY_BYTE: AtomicU8 = AtomicU8::new(0);

    const fn new() -> Self {
        Self {
            bytes: [Self::EMPTY_BYTE; N],
            len: AtomicU64::new(0),
        }
    }

    pub fn set(&self, text: &[u8]) {
        let mut end = text.len().min(N);
        while end > 0 && (text[end - 1] == b' ' || text[end - 1] == 0) {
            end -= 1;
        }
        for (slot, byte) in self.bytes.iter().zip(&text[..end]) {
            let printable = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte
            } else {
                b'?'
            };
            slot.store(printable, Ordering::Relaxed);
        }
        self.len.store(end as u64, Ordering::Release);
    }

    /// Copy into `out`; returns the used prefix as `&str` (ASCII by construction).
    pub fn get<'a>(&self, out: &'a mut [u8; N]) -> &'a str {
        let len = self.len.load(Ordering::Acquire) as usize;
        for (slot, byte) in out.iter_mut().zip(&self.bytes).take(len) {
            *slot = byte.load(Ordering::Relaxed);
        }
        core::str::from_utf8(&out[..len]).unwrap_or("")
    }
}

pub static NVME_MODEL: Text<40> = Text::new();
pub static CPU_BRAND: Text<48> = Text::new();
pub static NET_MAC: AtomicU64 = AtomicU64::new(0);
pub static NET_PRESENT: AtomicBool = AtomicBool::new(false);
pub static SATA_DISK: AtomicBool = AtomicBool::new(false);
pub static MEMORY_MIB: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_MAP_WX: AtomicBool = AtomicBool::new(false);

/// State of the local VPN tunnel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Vpn {
    NotConfigured = 0,
    Established = 1,
    Failed = 2,
}

static VPN: AtomicU8 = AtomicU8::new(Vpn::NotConfigured as u8);

#[allow(dead_code)] // written by the VPN tunnel
pub fn set_vpn(state: Vpn) {
    VPN.store(state as u8, Ordering::Release);
}

pub fn vpn() -> Vpn {
    match VPN.load(Ordering::Acquire) {
        1 => Vpn::Established,
        2 => Vpn::Failed,
        _ => Vpn::NotConfigured,
    }
}

/// Record the CPU brand string (CPUID 0x8000_0002..4).
pub fn record_cpu_brand() {
    let max = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if max < 0x8000_0004 {
        return;
    }
    let mut brand = [0_u8; 48];
    for (i, leaf) in (0x8000_0002_u32..=0x8000_0004).enumerate() {
        let r = core::arch::x86_64::__cpuid(leaf);
        for (j, word) in [r.eax, r.ebx, r.ecx, r.edx].iter().enumerate() {
            brand[i * 16 + j * 4..i * 16 + j * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
    }
    let start = brand.iter().position(|b| *b != b' ').unwrap_or(0);
    CPU_BRAND.set(&brand[start..]);
}
