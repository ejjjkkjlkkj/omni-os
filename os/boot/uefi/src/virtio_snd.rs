//! VirtIO sound output at the firmware stage - a third self-built audio backend, for virtual
//! machines whose audio device is `virtio-sound` rather than emulated Intel HDA or AC'97.
//!
//! UEFI defines no audio protocol, so the project builds its own drivers and [`crate::audio`]
//! tries them in turn. Modern VMs increasingly expose a paravirtual `virtio-sound` device
//! instead of emulating a hardware codec; this speaks to it directly, over the modern
//! VirtIO 1.x PCI transport, so the same pre-recorded clips and synthesized speech are heard
//! in those VMs too. Nothing here needs a USB stack - it is a PCI device like the others.
//!
//! The path: find the `virtio-sound` PCI function (vendor 0x1af4, device 0x1059), read its
//! VirtIO capability structures from PCI config space to locate the common-config, notify and
//! device-config MMIO windows, negotiate `VIRTIO_F_VERSION_1`, and set up two split
//! virtqueues - the control queue and the TX (playback) queue. To speak a clip it runs the
//! virtio-snd control handshake on stream 0 (SET_PARAMS, PREPARE, START), streams the PCM as
//! one TX buffer, then STOP/RELEASE. The 24 kHz mono clips are up-sampled to 48 kHz stereo
//! (each mono sample written twice, to both channels), the rate virtio-snd offers.
//!
//! Honest scope: this drives the one output stream (id 0) a default `virtio-sound` exposes,
//! polling the used rings rather than taking interrupts (there is no scheduler at this stage).
//! It is proven on QEMU (`-device virtio-sound-pci`), where the audio can be captured to a
//! WAV through the `wav` audiodev.

use uefi::boot;

use crate::aw_mark;

// ---- Port I/O and PCI configuration (mechanism #1) -----------------------------

unsafe fn outl(port: u16, value: u32) {
    // SAFETY: caller names a valid dword-wide port; a UEFI app runs at CPL0.
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
/// Read one config byte (the dword is read then the wanted byte selected).
unsafe fn pci_read8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    // SAFETY: as pci_read32.
    let dword = unsafe { pci_read32(bus, device, function, offset & 0xfc) };
    (dword >> ((offset & 3) * 8)) as u8
}

// ---- MMIO -----------------------------------------------------------------------

unsafe fn r8(addr: u64) -> u8 {
    // SAFETY: addr is inside an identity-mapped VirtIO BAR window.
    unsafe { (addr as *const u8).read_volatile() }
}
unsafe fn w8(addr: u64, value: u8) {
    // SAFETY: as r8.
    unsafe { (addr as *mut u8).write_volatile(value) }
}
unsafe fn r16(addr: u64) -> u16 {
    // SAFETY: as r8.
    unsafe { (addr as *const u16).read_volatile() }
}
unsafe fn w16(addr: u64, value: u16) {
    // SAFETY: as r8.
    unsafe { (addr as *mut u16).write_volatile(value) }
}
unsafe fn r32(addr: u64) -> u32 {
    // SAFETY: as r8.
    unsafe { (addr as *const u32).read_volatile() }
}
unsafe fn w32(addr: u64, value: u32) {
    // SAFETY: as r8.
    unsafe { (addr as *mut u32).write_volatile(value) }
}
unsafe fn w64(addr: u64, value: u64) {
    // A 64-bit VirtIO config field is written as two 32-bit halves, low then high.
    // SAFETY: as r8.
    unsafe {
        w32(addr, value as u32);
        w32(addr + 4, (value >> 32) as u32);
    }
}

// ---- VirtIO constants -----------------------------------------------------------

const VIRTIO_VENDOR: u16 = 0x1af4;
/// Modern VirtIO device id for sound: 0x1040 + VIRTIO_ID_SOUND (25).
const VIRTIO_SOUND_DEVICE: u16 = 0x1059;

// VirtIO PCI capability config types (in the vendor-specific capability's cfg_type byte).
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_DEVICE: u8 = 4;
const PCI_CAP_VENDOR: u8 = 0x09;

// virtio_pci_common_cfg field offsets.
const COMMON_DEVICE_FEATURE_SELECT: u64 = 0x00;
const COMMON_DEVICE_FEATURE: u64 = 0x04;
const COMMON_DRIVER_FEATURE_SELECT: u64 = 0x08;
const COMMON_DRIVER_FEATURE: u64 = 0x0c;
const COMMON_DEVICE_STATUS: u64 = 0x14;
const COMMON_QUEUE_SELECT: u64 = 0x16;
const COMMON_QUEUE_SIZE: u64 = 0x18;
const COMMON_QUEUE_ENABLE: u64 = 0x1c;
const COMMON_QUEUE_NOTIFY_OFF: u64 = 0x1e;
const COMMON_QUEUE_DESC: u64 = 0x20;
const COMMON_QUEUE_DRIVER: u64 = 0x28;
const COMMON_QUEUE_DEVICE: u64 = 0x30;

// Device status bits.
const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FEATURES_OK: u8 = 8;

/// `VIRTIO_F_VERSION_1` is feature bit 32 - bit 0 of the second feature dword.
const VIRTIO_F_VERSION_1_HI: u32 = 1;

// Descriptor flags.
const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

// virtio-snd control request codes.
const R_PCM_SET_PARAMS: u32 = 0x0101;
const R_PCM_PREPARE: u32 = 0x0102;
const R_PCM_RELEASE: u32 = 0x0103;
const R_PCM_START: u32 = 0x0104;
const R_PCM_STOP: u32 = 0x0105;
const S_OK: u32 = 0x8000;

// virtio-snd PCM format/rate enums.
const PCM_FMT_S16: u8 = 5;
const PCM_RATE_48000: u8 = 7;

/// The virtqueue indices a virtio-snd device defines.
const CONTROLQ: u16 = 0;
const TXQ: u16 = 2;

/// Fixed virtqueue size this driver uses (a power of two, <= the device's maximum). Small: one
/// message is in flight at a time and the largest chain is three descriptors.
const QSIZE: u16 = 8;

// ---- Virtqueue memory (page-aligned, identity-mapped statics) ------------------

/// One split-ring descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C, align(4096))]
struct Page4k([u8; 4096]);

// Two queues (control, tx), each with its own descriptor table, available ring and used ring.
static mut CTRL_DESC: Page4k = Page4k([0; 4096]);
static mut CTRL_AVAIL: Page4k = Page4k([0; 4096]);
static mut CTRL_USED: Page4k = Page4k([0; 4096]);
static mut TX_DESC: Page4k = Page4k([0; 4096]);
static mut TX_AVAIL: Page4k = Page4k([0; 4096]);
static mut TX_USED: Page4k = Page4k([0; 4096]);

/// Small buffers for control requests/responses and the TX message framing.
#[repr(C, align(64))]
struct SmallBuf([u8; 64]);
static mut CTRL_REQ: SmallBuf = SmallBuf([0; 64]);
static mut CTRL_RESP: SmallBuf = SmallBuf([0; 64]);
static mut TX_HDR: SmallBuf = SmallBuf([0; 64]);
static mut TX_STATUS: SmallBuf = SmallBuf([0; 64]);

/// Up-sampled 48 kHz stereo playback buffer. A mono sample becomes four 16-bit samples (two
/// frames, two channels): 2 MiB holds ~5.4 s, past the longest firmware line.
const AUDIO_BYTES: usize = 2 * 1024 * 1024;
#[repr(C, align(4096))]
struct AudioBuffer([u8; AUDIO_BYTES]);
static mut AUDIO: AudioBuffer = AudioBuffer([0; AUDIO_BYTES]);

const BYTES_PER_SEC: u32 = 48000 * 2 * 2;

// ---- A single split virtqueue --------------------------------------------------

/// One polled split virtqueue: the descriptor table, available ring and used ring addresses,
/// the device's notify address for it, and the driver's ring cursors.
struct VirtQueue {
    desc: u64,
    avail: u64,
    used: u64,
    notify: u64,
    avail_idx: u16,
    last_used: u16,
}

impl VirtQueue {
    /// Write descriptor `i` (addr, len, flags, next).
    fn set_desc(&self, i: u16, addr: u64, len: u32, flags: u16, next: u16) {
        let d = self.desc + u64::from(i) * core::mem::size_of::<Desc>() as u64;
        // SAFETY: `d` is inside the descriptor table's identity-mapped page.
        unsafe {
            w64(d, addr);
            w32(d + 8, len);
            w16(d + 12, flags);
            w16(d + 14, next);
        }
    }

    /// Publish descriptor chain head `head` on the available ring, notify the device, then poll
    /// the used ring until the chain is returned (or `budget` polls elapse, or `interrupted`).
    fn submit(&mut self, head: u16, mut interrupted: impl FnMut() -> bool, budget_ms: u32) -> bool {
        // avail: [flags u16][idx u16][ring u16 * QSIZE][used_event u16].
        let ring_slot = self.avail + 4 + u64::from(self.avail_idx % QSIZE) * 2;
        // SAFETY: rings are identity-mapped, indices are within QSIZE.
        unsafe {
            w16(ring_slot, head);
            self.avail_idx = self.avail_idx.wrapping_add(1);
            // Order the ring write before the idx update the device reads.
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            w16(self.avail + 2, self.avail_idx);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            // Notify the device that its queue has a new buffer.
            w16(self.notify, 0);
        }
        // used: [flags u16][idx u16][{id u32, len u32} * QSIZE][avail_event u16].
        let mut waited = 0u32;
        while waited < budget_ms {
            // SAFETY: reading the used index is side-effect free.
            let used_idx = unsafe { r16(self.used + 2) };
            if used_idx != self.last_used {
                self.last_used = used_idx;
                return true;
            }
            if interrupted() {
                return false;
            }
            boot::stall(core::time::Duration::from_millis(5));
            waited += 5;
        }
        false
    }
}

/// A brought-up virtio-sound device with its two queues, ready to speak.
pub struct Speaker {
    controlq: VirtQueue,
    txq: VirtQueue,
}

impl Speaker {
    /// Run one control request (its bytes already in `CTRL_REQ`, `len` long) and return whether
    /// the device answered `VIRTIO_SND_S_OK`. Uses a two-descriptor chain: the request
    /// (device-readable) then a 4-byte response (device-writable).
    fn control(&mut self, len: u32) -> bool {
        let req = core::ptr::addr_of!(CTRL_REQ) as u64;
        let resp = core::ptr::addr_of!(CTRL_RESP) as u64;
        self.controlq.set_desc(0, req, len, VRING_DESC_F_NEXT, 1);
        self.controlq.set_desc(1, resp, 4, VRING_DESC_F_WRITE, 0);
        if !self.controlq.submit(0, || false, 1000) {
            return false;
        }
        // SAFETY: CTRL_RESP is an identity-mapped static; the device wrote the status dword.
        let status = unsafe { r32(resp) };
        status == S_OK
    }

    /// Build a control request into `CTRL_REQ`: the 4-byte code, then a stream-id, then any
    /// extra bytes, and run it. Returns whether the device accepted it.
    fn pcm_control(&mut self, code: u32, extra: &[u8]) -> bool {
        let base = core::ptr::addr_of_mut!(CTRL_REQ) as *mut u8;
        // SAFETY: CTRL_REQ is 64 bytes; the request is code(4) + stream_id(4) + extra.
        unsafe {
            core::ptr::write_bytes(base, 0, 64);
            core::ptr::copy_nonoverlapping(code.to_le_bytes().as_ptr(), base, 4);
            // stream_id 0 at offset 4 is already zeroed.
            if !extra.is_empty() {
                core::ptr::copy_nonoverlapping(extra.as_ptr(), base.add(8), extra.len());
            }
        }
        self.control(8 + extra.len() as u32)
    }

    /// Configure stream 0 for `data_len` bytes of 48 kHz S16 stereo: SET_PARAMS then PREPARE
    /// then START. `data_len` is both the buffer and period size, so the whole clip is one
    /// period and streams as a single TX buffer.
    fn start_stream(&mut self, data_len: u32) -> bool {
        // virtio_snd_pcm_set_params tail after the 8-byte hdr: buffer_bytes(4) period_bytes(4)
        // features(4) channels(1) format(1) rate(1) padding(1).
        let mut params = [0u8; 16];
        params[0..4].copy_from_slice(&data_len.to_le_bytes());
        params[4..8].copy_from_slice(&data_len.to_le_bytes());
        params[8..12].copy_from_slice(&0u32.to_le_bytes());
        params[12] = 2; // channels
        params[13] = PCM_FMT_S16;
        params[14] = PCM_RATE_48000;
        self.pcm_control(R_PCM_SET_PARAMS, &params)
            && self.pcm_control(R_PCM_PREPARE, &[])
            && self.pcm_control(R_PCM_START, &[])
    }

    /// Stop and release stream 0 after playback, so the next clip can reconfigure it.
    fn stop_stream(&mut self) {
        self.pcm_control(R_PCM_STOP, &[]);
        self.pcm_control(R_PCM_RELEASE, &[]);
    }

    /// Play one 24 kHz mono PCM clip, blocking until it has finished or `interrupted` returns
    /// true (barge-in). Returns true when the device accepted and played the buffer.
    pub fn speak_until(&mut self, clip: &[u8], mut interrupted: impl FnMut() -> bool) -> bool {
        // Up-sample 24 kHz mono to 48 kHz stereo: each mono sample twice in time, both channels.
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
        let data_len = (mono_samples * 8) as u32; // 4 samples * 2 bytes

        if !self.start_stream(data_len) {
            aw_mark!("AW_UEFI_VIRTIO_SND_FAIL reason=start_stream");
            return false;
        }

        // TX message: xfer header { le32 stream_id = 0 } (readable), the PCM data (readable),
        // then a status struct (writable).
        let hdr = core::ptr::addr_of_mut!(TX_HDR) as *mut u8;
        let status = core::ptr::addr_of!(TX_STATUS) as u64;
        let data = core::ptr::addr_of!(AUDIO) as u64;
        // SAFETY: TX_HDR is 64 bytes; write the 4-byte stream id (0).
        unsafe {
            core::ptr::write_bytes(hdr, 0, 8);
        }
        self.txq.set_desc(0, hdr as u64, 4, VRING_DESC_F_NEXT, 1);
        self.txq.set_desc(1, data, data_len, VRING_DESC_F_NEXT, 2);
        self.txq.set_desc(2, status, 8, VRING_DESC_F_WRITE, 0);

        // The device returns the buffer on the used ring once the period has played, so waiting
        // for it times the clip. Budget the clip's duration plus a margin.
        let duration_ms = data_len / (BYTES_PER_SEC / 1000);
        let played = self.txq.submit(0, &mut interrupted, duration_ms + 300);

        self.stop_stream();
        if played {
            aw_mark!("AW_UEFI_VIRTIO_SND_PLAY bytes={}", data_len);
        }
        played
    }
}

// ---- Bring-up ------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

fn is_virtio_sound(loc: PciLocation) -> bool {
    // SAFETY: configuration reads have no side effects.
    let id = unsafe { pci_read32(loc.bus, loc.device, loc.function, 0x00) };
    (id & 0xffff) as u16 == VIRTIO_VENDOR && (id >> 16) as u16 == VIRTIO_SOUND_DEVICE
}

/// The 64-bit base address of BAR `index` (a memory BAR), or 0. Assumes a 64-bit MMIO BAR, as
/// modern VirtIO devices use; the firmware identity-maps it, so the physical address is usable.
fn bar_base(loc: PciLocation, index: u8) -> u64 {
    // SAFETY: reading BARs has no side effects.
    unsafe {
        let low = pci_read32(loc.bus, loc.device, loc.function, 0x10 + index * 4);
        let high = pci_read32(loc.bus, loc.device, loc.function, 0x10 + (index + 1) * 4);
        (u64::from(low & 0xffff_fff0)) | (u64::from(high) << 32)
    }
}

/// One located VirtIO capability window: the MMIO address it points at and its notify
/// multiplier (only meaningful for the notify capability).
struct CapWindow {
    address: u64,
    notify_multiplier: u32,
}

/// Walk the PCI capability list for the VirtIO vendor capability of `want` cfg_type and return
/// its MMIO window (BAR base + offset), or `None`.
fn find_cap(loc: PciLocation, want: u8) -> Option<CapWindow> {
    // SAFETY: configuration reads have no side effects.
    let mut ptr = unsafe { pci_read8(loc.bus, loc.device, loc.function, 0x34) } & 0xfc;
    let mut guard = 0;
    while ptr != 0 && guard < 48 {
        guard += 1;
        let cap_vndr = unsafe { pci_read8(loc.bus, loc.device, loc.function, ptr) };
        let cap_next = unsafe { pci_read8(loc.bus, loc.device, loc.function, ptr + 1) } & 0xfc;
        if cap_vndr == PCI_CAP_VENDOR {
            let cfg_type = unsafe { pci_read8(loc.bus, loc.device, loc.function, ptr + 3) };
            if cfg_type == want {
                let bar = unsafe { pci_read8(loc.bus, loc.device, loc.function, ptr + 4) };
                let offset = unsafe { pci_read32(loc.bus, loc.device, loc.function, ptr + 8) };
                let multiplier = if want == CFG_NOTIFY {
                    unsafe { pci_read32(loc.bus, loc.device, loc.function, ptr + 16) }
                } else {
                    0
                };
                let base = bar_base(loc, bar);
                if base != 0 {
                    return Some(CapWindow {
                        address: base + u64::from(offset),
                        notify_multiplier: multiplier,
                    });
                }
            }
        }
        ptr = cap_next;
    }
    None
}

/// Configure one virtqueue `index` of the given `size`: select it, cap its size, point the
/// device at the descriptor/avail/used rings, compute its notify address, and enable it.
#[allow(clippy::too_many_arguments)]
fn setup_queue(
    common: u64,
    notify_base: u64,
    notify_multiplier: u32,
    index: u16,
    desc: u64,
    avail: u64,
    used: u64,
) -> VirtQueue {
    // SAFETY: `common` is the identity-mapped common-config window.
    let notify_off = unsafe {
        w16(common + COMMON_QUEUE_SELECT, index);
        // Cap the queue to our fixed size (must be <= the device maximum, which QEMU's default
        // comfortably exceeds).
        w16(common + COMMON_QUEUE_SIZE, QSIZE);
        w64(common + COMMON_QUEUE_DESC, desc);
        w64(common + COMMON_QUEUE_DRIVER, avail);
        w64(common + COMMON_QUEUE_DEVICE, used);
        let off = r16(common + COMMON_QUEUE_NOTIFY_OFF);
        w16(common + COMMON_QUEUE_ENABLE, 1);
        off
    };
    VirtQueue {
        desc,
        avail,
        used,
        notify: notify_base + u64::from(notify_off) * u64::from(notify_multiplier),
        avail_idx: 0,
        last_used: 0,
    }
}

/// Find and bring up a virtio-sound device, ready to speak, or `None` (logging why) when there
/// is none - so [`crate::audio`] falls back to the next backend or the PC speaker.
pub fn bring_up() -> Option<Speaker> {
    let mut found: Option<PciLocation> = None;
    'scan: for bus in 0..=255u16 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let loc = PciLocation {
                    bus: bus as u8,
                    device,
                    function,
                };
                if is_virtio_sound(loc) {
                    found = Some(loc);
                    break 'scan;
                }
            }
        }
    }
    let loc = found?;

    // Enable memory space + bus mastering.
    // SAFETY: standard PCI command-register write.
    unsafe {
        let command = pci_read32(loc.bus, loc.device, loc.function, 0x04);
        pci_write32(loc.bus, loc.device, loc.function, 0x04, command | 0b110);
    }

    let common = find_cap(loc, CFG_COMMON)?.address;
    let notify = find_cap(loc, CFG_NOTIFY)?;
    let _device_cfg = find_cap(loc, CFG_DEVICE); // present but unused (default stream 0)

    // Reset, then the ACKNOWLEDGE/DRIVER handshake.
    // SAFETY: `common` is the identity-mapped common-config window.
    unsafe {
        w8(common + COMMON_DEVICE_STATUS, 0);
        // Spin until the device reports reset complete (status reads back 0).
        let mut budget = 1_000_000u32;
        while r8(common + COMMON_DEVICE_STATUS) != 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }
        w8(common + COMMON_DEVICE_STATUS, STATUS_ACKNOWLEDGE);
        w8(
            common + COMMON_DEVICE_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER,
        );

        // Confirm the device offers the modern interface (VIRTIO_F_VERSION_1, bit 32) before
        // driving it as one - a legacy-only device would need a different transport.
        w32(common + COMMON_DEVICE_FEATURE_SELECT, 1);
        if r32(common + COMMON_DEVICE_FEATURE) & VIRTIO_F_VERSION_1_HI == 0 {
            log::error!("AW_UEFI_VIRTIO_SND_FAIL reason=not_modern");
            return None;
        }

        // Negotiate features: accept only VIRTIO_F_VERSION_1 (bit 32).
        w32(common + COMMON_DRIVER_FEATURE_SELECT, 0);
        w32(common + COMMON_DRIVER_FEATURE, 0);
        w32(common + COMMON_DRIVER_FEATURE_SELECT, 1);
        w32(common + COMMON_DRIVER_FEATURE, VIRTIO_F_VERSION_1_HI);

        w8(
            common + COMMON_DEVICE_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
        );
        if r8(common + COMMON_DEVICE_STATUS) & STATUS_FEATURES_OK == 0 {
            log::error!("AW_UEFI_VIRTIO_SND_FAIL reason=features_ok");
            return None;
        }
    }

    let controlq = setup_queue(
        common,
        notify.address,
        notify.notify_multiplier,
        CONTROLQ,
        core::ptr::addr_of!(CTRL_DESC) as u64,
        core::ptr::addr_of!(CTRL_AVAIL) as u64,
        core::ptr::addr_of!(CTRL_USED) as u64,
    );
    let txq = setup_queue(
        common,
        notify.address,
        notify.notify_multiplier,
        TXQ,
        core::ptr::addr_of!(TX_DESC) as u64,
        core::ptr::addr_of!(TX_AVAIL) as u64,
        core::ptr::addr_of!(TX_USED) as u64,
    );

    // SAFETY: driver initialisation complete; announce we are live.
    unsafe {
        w8(
            common + COMMON_DEVICE_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );
    }
    aw_mark!("AW_UEFI_VIRTIO_SND_READY common=0x{common:x}");
    Some(Speaker { controlq, txq })
}
