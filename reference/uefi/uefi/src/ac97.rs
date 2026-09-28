//! AC'97 audio output at the firmware stage - a second, self-built audio backend so the
//! spoken screen reader works on machines whose sound is AC'97 rather than Intel HDA.
//!
//! UEFI has no standard audio protocol, so the project builds its own drivers rather than
//! depending on any one vendor: [`crate::hda`] drives Intel HDA, this drives AC'97 (the
//! older, still common codec, and what several virtual machines expose), and the PC
//! speaker ([`crate::sound`]) is the universal last resort. [`crate::audio`] tries them in
//! turn, so the same pre-recorded clips are spoken on whatever hardware a machine has.
//!
//! AC'97 is programmed through two I/O port windows the PCI BARs point at: BAR0 the Native
//! Audio Mixer (NAM: reset and volumes) and BAR1 the Native Audio Bus Master (NABM: the
//! PCM-out DMA engine, driven by a Buffer Descriptor List). The clips are 24 kHz mono; the
//! DAC runs at its default 48 kHz stereo, so each mono sample is written twice (up to
//! 48 kHz) to both channels, which plays at the right pitch without variable-rate audio.

use uefi::boot;

use crate::aw_mark;

// ---- Port I/O ------------------------------------------------------------------

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: caller names a valid byte-wide port; a UEFI app runs at CPL0.
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
unsafe fn outw(port: u16, value: u16) {
    // SAFETY: caller names a valid word-wide port.
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") value,
            options(nomem, nostack, preserves_flags));
    }
}
unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: caller names a valid word-wide port.
    unsafe {
        core::arch::asm!("in ax, dx", out("ax") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}
unsafe fn outl(port: u16, value: u32) {
    // SAFETY: caller names a valid dword-wide port.
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") value,
            options(nomem, nostack, preserves_flags));
    }
}
unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: caller names a valid dword-wide port.
    unsafe {
        core::arch::asm!("in eax, dx", out("eax") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}

// ---- PCI configuration (mechanism #1) ------------------------------------------

const PCI_CONFIG_ADDRESS: u16 = 0x0cf8;
const PCI_CONFIG_DATA: u16 = 0x0cfc;

fn pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | (u32::from(bus) << 16)
        | (u32::from(device) << 11)
        | (u32::from(function) << 8)
        | u32::from(offset & 0xfc)
}

unsafe fn pci_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    // SAFETY: CF8/CFC are the architected PCI configuration ports.
    unsafe {
        outl(
            PCI_CONFIG_ADDRESS,
            pci_address(bus, device, function, offset),
        );
        inl(PCI_CONFIG_DATA)
    }
}
unsafe fn pci_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    // SAFETY: CF8/CFC are the architected PCI configuration ports.
    unsafe {
        outl(
            PCI_CONFIG_ADDRESS,
            pci_address(bus, device, function, offset),
        );
        outl(PCI_CONFIG_DATA, value);
    }
}

// ---- AC'97 registers -----------------------------------------------------------

// Native Audio Mixer (NAM, BAR0).
const NAM_RESET: u16 = 0x00;
const NAM_MASTER_VOLUME: u16 = 0x02;
const NAM_PCM_OUT_VOLUME: u16 = 0x18;

// Native Audio Bus Master (NABM, BAR1): PCM-out box at offset 0x10.
const PO_BDBAR: u16 = 0x10; // buffer descriptor list base (dword)
const PO_LVI: u16 = 0x15; // last valid index (byte)
const PO_SR: u16 = 0x16; // status (word)
const PO_CR: u16 = 0x1b; // control (byte)

const CR_RUN: u8 = 1 << 0; // run/pause bus master
const CR_RESET: u8 = 1 << 1; // reset this box's registers
const SR_DMA_HALTED: u16 = 1 << 0;

/// One BDL entry is two dwords: the buffer's physical address, then the sample count
/// (low 16 bits) with the IOC/BUP flags in the high half.
const BDL_ENTRIES: usize = 32;
/// Max 16-bit samples one BDL entry can carry (the field is 16 bits, kept even).
const MAX_ENTRY_SAMPLES: usize = 0xfffe;
const BDL_IOC: u32 = 1 << 31;

#[repr(C, align(8))]
struct Bdl([u32; BDL_ENTRIES * 2]);
static mut BDL: Bdl = Bdl([0; BDL_ENTRIES * 2]);

/// Playback buffer for the up-sampled stereo audio. Sized for the longest clip: a mono
/// sample becomes four 16-bit samples (two frames, two channels), so 2 MiB holds ~5.4 s
/// of 48 kHz stereo, enough for the longest firmware line.
const AUDIO_BYTES: usize = 2 * 1024 * 1024;
#[repr(C, align(4096))]
struct AudioBuffer([u8; AUDIO_BYTES]);
static mut AUDIO: AudioBuffer = AudioBuffer([0; AUDIO_BYTES]);

/// Played stream is 48 kHz, 16-bit, stereo: 192000 bytes per second.
const BYTES_PER_SEC: u32 = 48000 * 2 * 2;

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

fn is_ac97(location: PciLocation) -> bool {
    // SAFETY: configuration reads have no side effects.
    let id = unsafe { pci_read32(location.bus, location.device, location.function, 0x00) };
    if id & 0xffff == 0xffff {
        return false;
    }
    let class = unsafe { pci_read32(location.bus, location.device, location.function, 0x08) };
    // Class 0x04 (multimedia), subclass 0x01 (audio device) - the AC'97 class, as opposed
    // to HDA's 0x04/0x03.
    (class >> 24) & 0xff == 0x04 && (class >> 16) & 0xff == 0x01
}

/// A brought-up AC'97 controller with its two I/O windows, ready to speak.
pub struct Speaker {
    nam: u16,
    nabm: u16,
}

impl Speaker {
    /// Play one 24 kHz mono PCM clip, blocking until it has finished or `interrupted`
    /// returns true (barge-in). Returns true when the DMA engine ran (audible speech).
    pub fn speak_until(&mut self, clip: &[u8], mut interrupted: impl FnMut() -> bool) -> bool {
        // Up-sample 24 kHz mono to 48 kHz stereo: each mono sample is written twice in time,
        // to both channels. One mono sample -> four 16-bit samples.
        let mono_samples = (clip.len() / 2).min(AUDIO_BYTES / 8);
        if mono_samples == 0 {
            return false;
        }
        let audio = core::ptr::addr_of_mut!(AUDIO) as *mut i16;
        for index in 0..mono_samples {
            let sample =
                crate::audio::scale(i16::from_le_bytes([clip[index * 2], clip[index * 2 + 1]]));
            // SAFETY: index*4+3 < AUDIO_BYTES/2, inside the buffer.
            unsafe {
                let base = index * 4;
                audio.add(base).write_volatile(sample);
                audio.add(base + 1).write_volatile(sample);
                audio.add(base + 2).write_volatile(sample);
                audio.add(base + 3).write_volatile(sample);
            }
        }
        let total_samples = mono_samples * 4; // 16-bit stereo samples
        let audio_phys = core::ptr::addr_of!(AUDIO) as u32;
        let bdl_phys = core::ptr::addr_of!(BDL) as u32;
        let bdl = core::ptr::addr_of_mut!(BDL) as *mut u32;

        // Fill the BDL: split the buffer into <=0xFFFE-sample chunks, one per entry.
        let mut remaining = total_samples;
        let mut sample_offset = 0usize;
        let mut entry = 0usize;
        while remaining > 0 && entry < BDL_ENTRIES {
            let chunk = remaining.min(MAX_ENTRY_SAMPLES);
            let addr = audio_phys + (sample_offset * 2) as u32;
            // SAFETY: BDL is an identity-mapped static; two dwords per entry fit.
            unsafe {
                bdl.add(entry * 2).write_volatile(addr);
                bdl.add(entry * 2 + 1)
                    .write_volatile(BDL_IOC | chunk as u32);
            }
            remaining -= chunk;
            sample_offset += chunk;
            entry += 1;
        }
        if entry == 0 {
            return false;
        }
        let last_index = (entry - 1) as u8;

        // SAFETY: NABM/NAM I/O ports are this controller's, from its PCI BARs.
        unsafe {
            // Reset the PCM-out box, then wait for the reset bit to clear.
            outb(self.nabm + PO_CR, CR_RESET);
            let mut budget = 1_000_000u32;
            while inb(self.nabm + PO_CR) & CR_RESET != 0 && budget > 0 {
                budget -= 1;
                core::hint::spin_loop();
            }
            // Full volume, unmuted (0x0000 = 0 dB attenuation).
            outw(self.nam + NAM_MASTER_VOLUME, 0x0000);
            outw(self.nam + NAM_PCM_OUT_VOLUME, 0x0000);
            // Program the descriptor list and start the engine.
            outl(self.nabm + PO_BDBAR, bdl_phys);
            outb(self.nabm + PO_LVI, last_index);
            outb(self.nabm + PO_CR, CR_RUN);
        }

        // Wait out the clip: its length plus a small margin, stopping early on barge-in or
        // when the DMA engine halts (finished).
        let duration_ms = (total_samples as u32 * 2) / (BYTES_PER_SEC / 1000);
        let mut waited = 0u32;
        let mut ran = false;
        while waited < duration_ms + 150 {
            if interrupted() {
                break;
            }
            boot::stall(core::time::Duration::from_millis(20));
            waited += 20;
            // SAFETY: reading the status word is side-effect free.
            let status = unsafe { inw(self.nabm + PO_SR) };
            // The box starts un-halted once running; treat that as "the engine ran".
            if status & SR_DMA_HALTED == 0 {
                ran = true;
            }
            if ran && status & SR_DMA_HALTED != 0 {
                break;
            }
        }
        // SAFETY: stop the engine on our own box.
        unsafe {
            outb(self.nabm + PO_CR, 0);
        }
        ran
    }
}

/// Enable I/O space + bus mastering and return the (NAM, NABM) I/O bases from BAR0/BAR1.
fn enable_bars(location: PciLocation) -> Option<(u16, u16)> {
    let PciLocation {
        bus,
        device,
        function,
    } = location;
    // SAFETY: enable I/O decode + bus mastering, then read the two I/O BARs.
    let (nam, nabm) = unsafe {
        let command = pci_read32(bus, device, function, 0x04);
        pci_write32(bus, device, function, 0x04, command | 0b101); // I/O space + bus master
        let bar0 = pci_read32(bus, device, function, 0x10);
        let bar1 = pci_read32(bus, device, function, 0x14);
        ((bar0 & 0xffff_fffc) as u16, (bar1 & 0xffff_fffc) as u16)
    };
    (nam != 0 && nabm != 0).then_some((nam, nabm))
}

/// Find and bring up an AC'97 controller, ready to speak, or `None` (logging why) when
/// there is no usable AC'97 so the caller can fall back to the PC speaker.
pub fn bring_up() -> Option<Speaker> {
    let mut found: Option<PciLocation> = None;
    'scan: for bus in 0..=255u16 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let location = PciLocation {
                    bus: bus as u8,
                    device,
                    function,
                };
                if is_ac97(location) {
                    found = Some(location);
                    break 'scan;
                }
            }
        }
    }
    let location = found?;
    let (nam, nabm) = enable_bars(location)?;

    // SAFETY: cold-reset the mixer by writing its reset register; NAM is this codec's.
    unsafe {
        outw(nam + NAM_RESET, 0x0000);
    }
    aw_mark!("AW_UEFI_AC97_READY nam=0x{:x} nabm=0x{:x}", nam, nabm);
    Some(Speaker { nam, nabm })
}
