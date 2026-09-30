//! Intel High Definition Audio driver: the machine's real audio path, and the
//! foundation for spoken output (roadmap Phase 5 "Speech service"; Phase 6 "Audio
//! stack"). This is what lets a blind user *hear* the screen reader, not only have
//! its words written to a console.
//!
//! HDA is a memory-mapped controller (a BAR0 register block) that talks to one or
//! more codecs over a command ring (CORB) and a response ring (RIRB) in RAM, and
//! moves audio samples by DMA through stream descriptors. This module brings the
//! controller out of reset, stands up the CORB/RIRB rings, finds the codec, and
//! reads the codec's identity back over that ring - real data the codec produced,
//! proving the whole command/response path works before a single sample is played.
//!
//! Everything the controller touches by DMA lives in page-aligned `static`s the
//! identity map covers 1:1, so a structure's virtual address is also the physical
//! address handed to the hardware, exactly as the NVMe and AHCI drivers do it.
//! Bring-up is side-effect-free (it plays nothing), so it runs on the normal boot
//! path; a machine with no HDA controller reports `AW_HDA_UNAVAILABLE` and the
//! proof is skipped, never failed.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use aw_x86_paging::PageTableFlags;

use crate::page_mapper::map_page;
use crate::{debug_write, debug_write_hex_u64, debug_write_u64};

// ---- PCI mechanism #1 (CF8/CFC), as the other device drivers use ---------

unsafe fn outl(port: u16, value: u32) {
    // SAFETY: the caller names a valid dword-wide port.
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: the caller names a valid dword-wide port.
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

// ---- MMIO on the controller's BAR0 register block ------------------------

// Used by the stream-playback stage (stream status/control are byte registers).
#[allow(dead_code)]
unsafe fn mmio_read8(base: u64, offset: u64) -> u8 {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 window.
    unsafe { ((base + offset) as *const u8).read_volatile() }
}

unsafe fn mmio_write8(base: u64, offset: u64, value: u8) {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 window.
    unsafe { ((base + offset) as *mut u8).write_volatile(value) };
}

unsafe fn mmio_read16(base: u64, offset: u64) -> u16 {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 window.
    unsafe { ((base + offset) as *const u16).read_volatile() }
}

unsafe fn mmio_write16(base: u64, offset: u64, value: u16) {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 window.
    unsafe { ((base + offset) as *mut u16).write_volatile(value) };
}

unsafe fn mmio_read32(base: u64, offset: u64) -> u32 {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 window.
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

unsafe fn mmio_write32(base: u64, offset: u64, value: u32) {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 window.
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
}

// ---- HDA controller registers (Intel HDA spec, section 3.3) --------------

const REG_GCAP: u64 = 0x00; // global capabilities (u16)
const REG_GCTL: u64 = 0x08; // global control (u32)
const REG_STATESTS: u64 = 0x0e; // state change status (u16): codec presence
const REG_CORBLBASE: u64 = 0x40;
const REG_CORBUBASE: u64 = 0x44;
const REG_CORBWP: u64 = 0x48; // u16
const REG_CORBRP: u64 = 0x4a; // u16
const REG_CORBCTL: u64 = 0x4c; // u8
const REG_CORBSIZE: u64 = 0x4e; // u8
const REG_RIRBLBASE: u64 = 0x50;
const REG_RIRBUBASE: u64 = 0x54;
const REG_RIRBWP: u64 = 0x58; // u16
const REG_RINTCNT: u64 = 0x5a; // u16
const REG_RIRBCTL: u64 = 0x5c; // u8
const REG_RIRBSTS: u64 = 0x5d; // u8
const REG_RIRBSIZE: u64 = 0x5e; // u8

const GCTL_CRST: u32 = 1 << 0; // controller reset (0 = reset asserted)
const CORBRP_RST: u16 = 1 << 15; // CORB read-pointer reset
const CORBCTL_RUN: u8 = 1 << 1; // CORB DMA engine run
const RIRBWP_RST: u16 = 1 << 15; // RIRB write-pointer reset
const RIRBCTL_DMAEN: u8 = 1 << 1; // RIRB DMA engine enable
const RIRBSTS_INTFL: u8 = 1 << 0; // response interrupt flag (write 1 to clear)

/// 256 ring entries, the size every real controller and QEMU supports.
const RING_ENTRIES: u16 = 256;
/// CORBSIZE/RIRBSIZE value selecting 256 entries (bits 1:0 = 2).
const RING_SIZE_256: u8 = 0x02;

/// 12-bit verb "get parameter", used to read a node's capabilities/identity.
const VERB_GET_PARAMETER: u32 = 0xf00;
/// Parameter 0x00: vendor id (upper 16 bits) and device id (lower 16 bits).
const PARAM_VENDOR_ID: u32 = 0x00;
/// Parameter 0x04: subordinate node count - starting node (bits 23:16) and count
/// (bits 7:0).
const PARAM_SUBNODE_COUNT: u32 = 0x04;
/// Parameter 0x05: function group type; 0x01 in the low byte is an audio group.
const PARAM_FUNCTION_GROUP_TYPE: u32 = 0x05;
/// Parameter 0x09: audio widget capabilities; type is bits 23:20.
const PARAM_WIDGET_CAP: u32 = 0x09;
/// Parameter 0x0C: pin capabilities; bit 4 means the pin can drive output.
const PARAM_PIN_CAP: u32 = 0x0c;

/// Widget type (bits 23:20 of the widget-capabilities parameter).
const WIDGET_AUDIO_OUTPUT: u32 = 0x0; // a DAC
const WIDGET_PIN_COMPLEX: u32 = 0x4; // a physical jack/speaker

/// 4-bit verb "set converter format" (16-bit payload = the stream format).
const VERB4_SET_FORMAT: u32 = 0x2;
/// 4-bit verb "set amplifier gain/mute" (16-bit payload).
const VERB4_SET_AMP: u32 = 0x3;
/// 12-bit verb "set power state" (payload 0 = fully on, D0).
const VERB_SET_POWER_STATE: u32 = 0x705;
/// 12-bit verb "set converter stream/channel" (payload = stream<<4 | channel).
const VERB_SET_STREAM_CHANNEL: u32 = 0x706;
/// 12-bit verb "set pin widget control" (payload bit6 = output enable).
const VERB_SET_PIN_CONTROL: u32 = 0x707;
/// 12-bit verb "set EAPD/BTL enable" (payload bit1 = EAPD, external amp).
const VERB_SET_EAPD: u32 = 0x70c;

/// Pin control payload: enable output (bit 6).
const PIN_CONTROL_OUT_ENABLE: u32 = 1 << 6;
/// EAPD payload: enable the external amplifier (bit 1).
const EAPD_ENABLE: u32 = 1 << 1;
/// Amp payload: set output amp, both channels, unmuted, at a moderate gain.
const AMP_OUT_UNMUTE: u32 = (1 << 15) | (1 << 13) | (1 << 12) | 0x2a;

/// Stream format: 48 kHz, 16-bit, two channels (base 48k, x1, /1, 16-bit, 2ch).
const STREAM_FORMAT: u16 = 0x0011;
/// The stream tag we assign to our one output stream (nonzero; not the index).
const STREAM_TAG: u8 = 1;

// ---- Stream descriptor registers (relative to a stream's own base) --------

const SD_CTL: u64 = 0x00; // control (3 bytes): SRST=bit0, RUN=bit1
const SD_LPIB: u64 = 0x04; // link position in buffer (u32)
const SD_CBL: u64 = 0x08; // cyclic buffer length (u32)
const SD_LVI: u64 = 0x0c; // last valid BDL index (u16)
const SD_FMT: u64 = 0x12; // format (u16)
const SD_BDPL: u64 = 0x18; // BDL pointer low (u32)
const SD_BDPU: u64 = 0x1c; // BDL pointer high (u32)

const SDCTL_SRST: u8 = 1 << 0; // stream reset
const SDCTL_RUN: u8 = 1 << 1; // stream run

/// First stream descriptor register block, and the per-stream stride.
const STREAM_BASE: u64 = 0x80;
const STREAM_STRIDE: u64 = 0x20;

/// The bring-up map identity-covers the low 4 GiB; a BAR above it is mapped first.
const IDENTITY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
/// Pages of the BAR0 register block to map: registers plus the stream descriptors.
const MMIO_PAGES: u64 = 4;
const PAGE_SIZE: u64 = 4096;

// ---- DMA rings, page-aligned identity-mapped statics ---------------------

/// One 4 KiB page. CORB (256 * 4 = 1024 bytes) and RIRB (256 * 8 = 2048 bytes)
/// each fit in one, well within their required 128-byte alignment.
#[repr(C, align(4096))]
struct Page([u8; 4096]);

static mut CORB: Page = Page([0; 4096]);
static mut RIRB: Page = Page([0; 4096]);

/// The Buffer Descriptor List for the output stream: one entry is enough, but the
/// block is page-aligned (well over the 128-byte requirement) and identity-mapped.
static mut BDL: Page = Page([0; 4096]);

/// Bytes of PCM the output stream plays: 48 kHz, 16-bit, stereo. 98304 bytes is a
/// clean multiple of the 4-byte frame and long enough (~0.5 s) to hear and to see
/// the link position advance well past zero.
const AUDIO_BYTES: usize = 98304;

#[repr(C, align(4096))]
struct AudioBuffer([u8; AUDIO_BYTES]);

static mut AUDIO: AudioBuffer = AudioBuffer([0; AUDIO_BYTES]);

/// 48 kHz, 16-bit, stereo: the one rate every HD Audio codec must support (HDA 1.0a,
/// 7.3.4.7), and what the codec is set to for spoken output. Speech sources (24 kHz recorded
/// clips, the 32 kHz voice) are resampled to it when staged; a codec left at 24 kHz mono
/// produces noise on hardware and hypervisors that do not implement that format (VMware).
const SPEECH_FORMAT: u16 = 0x0011;
/// Output rate of [`SPEECH_FORMAT`].
const OUTPUT_RATE: u64 = 48_000;

/// The buffer speech streams from by DMA: about 11 s of 48 kHz stereo 16-bit audio;
/// page-aligned and identity-mapped like the other DMA statics.
const SPEECH_BYTES: usize = 2 * 1024 * 1024;

#[repr(C, align(4096))]
struct SpeechBuffer([u8; SPEECH_BYTES]);

static mut SPEECH: SpeechBuffer = SpeechBuffer([0; SPEECH_BYTES]);

/// Output stream 0's MMIO block, retained after bring-up so the menu can play
/// clips without re-walking the codec each time.
struct SpeechEngine {
    stream_base: u64,
}

struct SpeechCell(UnsafeCell<Option<SpeechEngine>>);
// SAFETY: written once during single-core bring-up, read afterwards on the BSP.
unsafe impl Sync for SpeechCell {}
static SPEECH_ENGINE: SpeechCell = SpeechCell(UnsafeCell::new(None));

/// Set once the codec is configured for speech and the engine is stored.
static SPEECH_READY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

/// Is the device at this location an HDA controller (class 04h, subclass 03h)?
fn is_hda(location: PciLocation) -> bool {
    // SAFETY: configuration reads have no side effects.
    let id = unsafe { pci_read32(location.bus, location.device, location.function, 0x00) };
    if id & 0xffff == 0xffff {
        return false;
    }
    let class = unsafe { pci_read32(location.bus, location.device, location.function, 0x08) };
    (class >> 24) & 0xff == 0x04 && (class >> 16) & 0xff == 0x03
}

/// Enable memory space + bus mastering and return the controller's BAR0 base.
fn map_bar0(location: PciLocation) -> Option<u64> {
    let PciLocation {
        bus,
        device,
        function,
    } = location;
    // SAFETY: enable MMIO + bus mastering, then read BAR0. HDA BAR0 is a 64-bit
    // memory BAR; read both halves.
    let base = unsafe {
        let command = pci_read32(bus, device, function, 0x04);
        pci_write32(bus, device, function, 0x04, command | 0b110);
        let low = pci_read32(bus, device, function, 0x10);
        let high = pci_read32(bus, device, function, 0x14);
        (u64::from(low & 0xffff_fff0)) | (u64::from(high) << 32)
    };
    if base == 0 {
        return None;
    }
    if base >= IDENTITY_LIMIT {
        let flags = PageTableFlags::WRITABLE
            .union(PageTableFlags::CACHE_DISABLE)
            .union(PageTableFlags::NO_EXECUTE);
        for page in 0..MMIO_PAGES {
            let addr = base + page * PAGE_SIZE;
            // SAFETY: identity-map one device-MMIO page (virt == phys); the frame
            // is the controller's own BAR, owned by this driver while it runs.
            if unsafe { map_page(addr, addr, flags) }.is_err() {
                return None;
            }
        }
    }
    Some(base)
}

/// A brought-up HDA controller: its BAR0 base and the codec it found.
struct Controller {
    base: u64,
    codec: u8,
    /// Number of input streams, which precede the output streams in the register
    /// file: output stream 0's descriptor is at `STREAM_BASE + input_streams * stride`.
    input_streams: u8,
    /// Our own RIRB read pointer (the hardware only exposes the write pointer).
    rirb_read: u16,
}

/// Take the controller out of reset. Returns false if it never reports ready.
fn reset(base: u64) -> bool {
    // SAFETY: BAR0 is the controller's identity-mapped MMIO window.
    unsafe {
        // Assert reset (CRST = 0) and wait for the controller to acknowledge.
        mmio_write32(base, REG_GCTL, mmio_read32(base, REG_GCTL) & !GCTL_CRST);
        let mut budget = 10_000_000u32;
        while mmio_read32(base, REG_GCTL) & GCTL_CRST != 0 {
            budget -= 1;
            if budget == 0 {
                return false;
            }
            core::hint::spin_loop();
        }
        // De-assert reset (CRST = 1) and wait for the controller to come up.
        mmio_write32(base, REG_GCTL, mmio_read32(base, REG_GCTL) | GCTL_CRST);
        let mut budget = 10_000_000u32;
        while mmio_read32(base, REG_GCTL) & GCTL_CRST == 0 {
            budget -= 1;
            if budget == 0 {
                return false;
            }
            core::hint::spin_loop();
        }
    }
    true
}

/// Stand up the CORB (command) and RIRB (response) rings and start their DMA.
fn setup_rings(base: u64) {
    let corb_phys = core::ptr::addr_of!(CORB) as u64;
    let rirb_phys = core::ptr::addr_of!(RIRB) as u64;

    // SAFETY: MMIO on a controller that exists; the ring bases are page-aligned
    // statics the identity map covers 1:1.
    unsafe {
        // Stop both engines before reprogramming them.
        mmio_write8(base, REG_CORBCTL, 0);
        mmio_write8(base, REG_RIRBCTL, 0);

        // CORB: 256 entries, base address, read pointer reset, write pointer 0.
        mmio_write8(base, REG_CORBSIZE, RING_SIZE_256);
        mmio_write32(base, REG_CORBLBASE, corb_phys as u32);
        mmio_write32(base, REG_CORBUBASE, (corb_phys >> 32) as u32);
        mmio_write16(base, REG_CORBRP, CORBRP_RST);
        let mut budget = 1_000_000u32;
        while mmio_read16(base, REG_CORBRP) & CORBRP_RST == 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }
        mmio_write16(base, REG_CORBRP, 0);
        mmio_write16(base, REG_CORBWP, 0);

        // RIRB: 256 entries, base address, write pointer reset, DMA enabled.
        mmio_write8(base, REG_RIRBSIZE, RING_SIZE_256);
        mmio_write32(base, REG_RIRBLBASE, rirb_phys as u32);
        mmio_write32(base, REG_RIRBUBASE, (rirb_phys >> 32) as u32);
        mmio_write16(base, REG_RIRBWP, RIRBWP_RST);
        mmio_write16(base, REG_RINTCNT, 0xff);
        mmio_write8(base, REG_RIRBCTL, RIRBCTL_DMAEN);

        // Start the CORB DMA engine.
        mmio_write8(base, REG_CORBCTL, CORBCTL_RUN);
    }
}

impl Controller {
    /// Send one verb to `codec`/`nid` and return the codec's 32-bit response, or
    /// an error if no response arrived. Polls the RIRB write pointer; no interrupts.
    fn command(&mut self, nid: u8, verb: u32, payload: u32) -> Result<u32, &'static str> {
        self.send_raw(build_verb(self.codec, nid, verb, payload))
    }

    /// Place one fully-built verb DWORD on the CORB, ring the write pointer, and
    /// return the codec's response from the RIRB. Polls; no interrupts.
    fn send_raw(&mut self, command: u32) -> Result<u32, &'static str> {
        let corb = core::ptr::addr_of_mut!(CORB) as *mut u32;
        // SAFETY: CORB is a live identity-mapped ring; advance the write pointer by
        // one and place the verb at the new slot, as the controller expects.
        unsafe {
            let next = (mmio_read16(self.base, REG_CORBWP) + 1) % RING_ENTRIES;
            corb.add(next as usize).write_volatile(command);
            mmio_write16(self.base, REG_CORBWP, next);

            let mut budget = 10_000_000u32;
            loop {
                let write = mmio_read16(self.base, REG_RIRBWP) & (RING_ENTRIES - 1);
                if write != self.rirb_read {
                    break;
                }
                budget -= 1;
                if budget == 0 {
                    debug_write("AW_HDA_DIAG corbwp=");
                    debug_write_u64(u64::from(mmio_read16(self.base, REG_CORBWP)));
                    debug_write(" corbrp=");
                    debug_write_u64(u64::from(mmio_read16(self.base, REG_CORBRP)));
                    debug_write(" rirbwp=");
                    debug_write_u64(u64::from(mmio_read16(self.base, REG_RIRBWP)));
                    debug_write(" rirbrd=");
                    debug_write_u64(u64::from(self.rirb_read));
                    debug_write("\n");
                    return Err("no_response");
                }
                core::hint::spin_loop();
            }

            self.rirb_read = (self.rirb_read + 1) % RING_ENTRIES;
            let rirb = core::ptr::addr_of!(RIRB) as *const u32;
            let response = rirb.add(self.rirb_read as usize * 2).read_volatile();

            // Acknowledge the response interrupt flag so the status is clean.
            mmio_write8(self.base, REG_RIRBSTS, RIRBSTS_INTFL);
            Ok(response)
        }
    }

    /// Read a node's `GET_PARAMETER` value.
    fn get_parameter(&mut self, nid: u8, parameter: u32) -> Result<u32, &'static str> {
        self.command(nid, VERB_GET_PARAMETER, parameter)
    }

    /// Send a 4-bit-verb / 16-bit-payload command (format, amp), ignoring the
    /// response the codec still returns.
    fn command16(&mut self, nid: u8, verb4: u32, payload: u16) -> Result<(), &'static str> {
        let value = (u32::from(self.codec) << 28)
            | (u32::from(nid) << 20)
            | ((verb4 & 0xf) << 16)
            | u32::from(payload);
        self.send_raw(value).map(|_| ())
    }

    /// Send a 12-bit-verb / 8-bit-payload command, ignoring the response.
    fn set(&mut self, nid: u8, verb: u32, payload: u32) -> Result<(), &'static str> {
        self.command(nid, verb, payload).map(|_| ())
    }

    /// Widget type (bits 23:20 of the widget-capabilities parameter) for `nid`.
    fn widget_type(&mut self, nid: u8) -> Result<u32, &'static str> {
        Ok((self.get_parameter(nid, PARAM_WIDGET_CAP)? >> 20) & 0xf)
    }

    /// The MMIO base of output stream 0's descriptor block.
    fn output_stream_base(&self) -> u64 {
        self.base + STREAM_BASE + u64::from(self.input_streams) * STREAM_STRIDE
    }
}

/// Build a CORB verb DWORD for the 12-bit-verb / 8-bit-payload command form.
fn build_verb(codec: u8, nid: u8, verb: u32, payload: u32) -> u32 {
    (u32::from(codec) << 28) | (u32::from(nid) << 20) | ((verb & 0xfff) << 8) | (payload & 0xff)
}

/// Find, reset and initialise an HDA controller, returning it with a codec found.
fn init() -> Option<Controller> {
    let mut found: Option<PciLocation> = None;
    'scan: for bus in 0..=255u16 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let location = PciLocation {
                    bus: bus as u8,
                    device,
                    function,
                };
                if is_hda(location) {
                    found = Some(location);
                    break 'scan;
                }
            }
        }
    }

    let Some(location) = found else {
        debug_write("AW_HDA_UNAVAILABLE reason=no_controller\n");
        return None;
    };
    debug_write("AW_HDA_FOUND bus=");
    debug_write_u64(u64::from(location.bus));
    debug_write(" device=");
    debug_write_u64(u64::from(location.device));
    debug_write(" function=");
    debug_write_u64(u64::from(location.function));
    debug_write("\n");

    let base = map_bar0(location)?;
    debug_write("AW_HDA_BAR0 base=");
    debug_write_hex_u64(base);
    debug_write("\n");

    if !reset(base) {
        debug_write("AW_HDA_FAIL reason=reset\n");
        return None;
    }
    // SAFETY: BAR0 is the controller's identity-mapped MMIO window.
    let gcap = unsafe { mmio_read16(base, REG_GCAP) };
    debug_write("AW_HDA_RESET_OK oss=");
    debug_write_u64(u64::from((gcap >> 12) & 0xf)); // output stream count
    debug_write(" iss=");
    debug_write_u64(u64::from((gcap >> 8) & 0xf)); // input stream count
    debug_write("\n");

    setup_rings(base);

    // The controller needs a moment after reset before codecs report present.
    // SAFETY: reading STATESTS has no side effects.
    let statests = unsafe {
        let mut budget = 1_000_000u32;
        let mut bits = mmio_read16(base, REG_STATESTS);
        while bits == 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
            bits = mmio_read16(base, REG_STATESTS);
        }
        bits
    };
    if statests == 0 {
        debug_write("AW_HDA_FAIL reason=no_codec\n");
        return None;
    }
    let codec = statests.trailing_zeros() as u8;
    debug_write("AW_HDA_CODEC_PRESENT addr=");
    debug_write_u64(u64::from(codec));
    debug_write("\n");

    Some(Controller {
        base,
        codec,
        input_streams: ((gcap >> 8) & 0xf) as u8,
        rirb_read: 0,
    })
}

/// Fill the PCM buffer with silence, 48 kHz 16-bit stereo. The playback proof needs the
/// controller to stream bytes by DMA (its link position must advance), not an audible
/// tone: a test beep at every boot is noise to the person listening.
fn fill_tone() {
    let audio = core::ptr::addr_of_mut!(AUDIO) as *mut i16;
    let frames = AUDIO_BYTES / 4; // 2 channels * 2 bytes
    for frame in 0..frames {
        let sample: i16 = 0;
        // SAFETY: `audio` is the identity-mapped PCM static; `frame*2+1 < frames*2`
        // stays inside its `AUDIO_BYTES` bounds.
        unsafe {
            audio.add(frame * 2).write_volatile(sample); // left
            audio.add(frame * 2 + 1).write_volatile(sample); // right
        }
    }
}

/// A DAC and an output pin discovered in the codec's widget graph.
struct OutputPath {
    dac: u8,
    pin: u8,
}

/// Walk the codec's function groups and widgets to find an audio-output converter
/// (DAC) and an output-capable pin complex to route it to.
fn find_output(controller: &mut Controller) -> Result<OutputPath, &'static str> {
    let root = controller.get_parameter(0, PARAM_SUBNODE_COUNT)?;
    let first_group = ((root >> 16) & 0xff) as u8;
    let group_count = (root & 0xff) as u8;

    for group in 0..group_count {
        let nid = first_group + group;
        let kind = controller.get_parameter(nid, PARAM_FUNCTION_GROUP_TYPE)?;
        if kind & 0xff != 0x01 {
            continue; // not an audio function group
        }
        // Power the function group fully on before walking it.
        controller.set(nid, VERB_SET_POWER_STATE, 0)?;

        let widgets = controller.get_parameter(nid, PARAM_SUBNODE_COUNT)?;
        let first_widget = ((widgets >> 16) & 0xff) as u8;
        let widget_count = (widgets & 0xff) as u8;

        let mut dac: Option<u8> = None;
        let mut pin: Option<u8> = None;
        for index in 0..widget_count {
            let widget = first_widget + index;
            let widget_type = controller.widget_type(widget)?;
            if widget_type == WIDGET_AUDIO_OUTPUT && dac.is_none() {
                dac = Some(widget);
            } else if widget_type == WIDGET_PIN_COMPLEX && pin.is_none() {
                let caps = controller.get_parameter(widget, PARAM_PIN_CAP)?;
                if caps & (1 << 4) != 0 {
                    pin = Some(widget);
                }
            }
        }
        if let (Some(dac), Some(pin)) = (dac, pin) {
            return Ok(OutputPath { dac, pin });
        }
    }
    Err("no_output_path")
}

/// Configure the codec to play our stream: power, format and stream tag on the
/// DAC, output enable and unmute on the pin.
fn configure_codec(controller: &mut Controller, path: &OutputPath) -> Result<(), &'static str> {
    // DAC: fully on, our stream format, stream tag with channel 0, output unmuted.
    controller.set(path.dac, VERB_SET_POWER_STATE, 0)?;
    controller.command16(path.dac, VERB4_SET_FORMAT, STREAM_FORMAT)?;
    controller.set(
        path.dac,
        VERB_SET_STREAM_CHANNEL,
        u32::from(STREAM_TAG) << 4,
    )?;
    controller.command16(path.dac, VERB4_SET_AMP, AMP_OUT_UNMUTE as u16)?;

    // Pin: fully on, output driver enabled, external amp on, output unmuted.
    controller.set(path.pin, VERB_SET_POWER_STATE, 0)?;
    controller.set(path.pin, VERB_SET_PIN_CONTROL, PIN_CONTROL_OUT_ENABLE)?;
    controller.set(path.pin, VERB_SET_EAPD, EAPD_ENABLE)?;
    controller.command16(path.pin, VERB4_SET_AMP, AMP_OUT_UNMUTE as u16)?;
    Ok(())
}

/// Program output stream 0's descriptor with a one-entry BDL over the PCM buffer
/// and start it running.
fn start_stream(controller: &Controller) {
    let stream = controller.output_stream_base();
    let bdl_phys = core::ptr::addr_of!(BDL) as u64;
    let audio_phys = core::ptr::addr_of!(AUDIO) as u64;

    let bdl = core::ptr::addr_of_mut!(BDL) as *mut u32;
    // SAFETY: BDL is the identity-mapped descriptor page; AUDIO is the DMA buffer.
    let lvi = unsafe { write_bdl(bdl, audio_phys, AUDIO_BYTES) };

    // SAFETY: `stream` is inside the identity-mapped BAR0 register file.
    unsafe {
        // Reset the stream, then release it, before programming.
        mmio_write8(stream, SD_CTL, SDCTL_SRST);
        let mut budget = 1_000_000u32;
        while mmio_read8(stream, SD_CTL) & SDCTL_SRST == 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }
        mmio_write8(stream, SD_CTL, 0);
        let mut budget = 1_000_000u32;
        while mmio_read8(stream, SD_CTL) & SDCTL_SRST != 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }

        // Buffer length, last-valid-index (two entries), format, BDL pointer.
        mmio_write32(stream, SD_CBL, AUDIO_BYTES as u32);
        mmio_write16(stream, SD_LVI, lvi);
        mmio_write16(stream, SD_FMT, STREAM_FORMAT);
        mmio_write32(stream, SD_BDPL, bdl_phys as u32);
        mmio_write32(stream, SD_BDPU, (bdl_phys >> 32) as u32);

        // Tag the stream (bits 20-23 of the 3-byte control) and set RUN.
        mmio_write8(stream, SD_CTL + 2, STREAM_TAG << 4);
        mmio_write8(stream, SD_CTL, mmio_read8(stream, SD_CTL) | SDCTL_RUN);
    }
}

/// Copy a speech clip into the identity-mapped speech buffer, truncated to it and
/// aligned to the 2-byte mono frame. Returns the number of PCM bytes staged.
/// Stage mono 16-bit PCM at `rate` Hz into the speech buffer as 48 kHz stereo, by linear
/// interpolation in integer arithmetic. Returns the staged byte count.
fn stage_pcm(pcm: &[u8], rate: u32) -> usize {
    let samples = pcm.len() / 2;
    if samples == 0 || rate == 0 {
        return 0;
    }
    let rate = u64::from(rate);
    let sample = |i: usize| -> i64 {
        let i = i.min(samples - 1);
        i64::from(i16::from_le_bytes([pcm[2 * i], pcm[2 * i + 1]]))
    };
    let frames = ((samples as u64 * OUTPUT_RATE / rate) as usize)
        .min((SPEECH_BYTES - TAIL_SILENCE_BYTES) / 4);
    let dst = core::ptr::addr_of_mut!(SPEECH) as *mut u8;
    for n in 0..frames {
        let position = n as u64 * rate;
        let index = (position / OUTPUT_RATE) as usize;
        let fraction = (position % OUTPUT_RATE) as i64;
        let value = (sample(index) * (OUTPUT_RATE as i64 - fraction)
            + sample(index + 1) * fraction)
            / OUTPUT_RATE as i64;
        let bytes = (value as i16).to_le_bytes();
        // SAFETY: SPEECH is the identity-mapped PCM static; 4 * frames <= SPEECH_BYTES.
        unsafe {
            for (k, byte) in [bytes[0], bytes[1], bytes[0], bytes[1]]
                .into_iter()
                .enumerate()
            {
                dst.add(4 * n + k).write_volatile(byte);
            }
        }
    }
    // SAFETY: as above; the tail stays inside SPEECH_BYTES by the bound on `frames`.
    unsafe {
        core::ptr::write_bytes(dst.add(frames * 4), 0, TAIL_SILENCE_BYTES);
    }
    frames * 4
}

/// A recorded clip (24 kHz mono 16-bit).
fn stage_clip(clip: &[u8]) -> usize {
    stage_pcm(clip, 24_000)
}

/// Silence appended after every utterance (100 ms at 48 kHz stereo): the stream is cyclic,
/// so whatever the codec fetches between the end of the speech and the stop is silence, never
/// the start of the buffer again (heard as a click or a beep).
const TAIL_SILENCE_BYTES: usize = 48_000 / 10 * 4;

/// Describe `len` bytes at `phys` as a two-entry buffer descriptor list at `bdl` - the HD Audio
/// specification requires at least two entries (1.0a, 3.6.2); a one-entry list is tolerated
/// by QEMU but not by other controllers (VMware, real hardware), which then stream garbage.
/// Entries are 128-byte aligned. Returns the last valid index.
///
/// # Safety
/// `bdl` points to a writable, identity-mapped descriptor page; `phys..phys+len` is a DMA
/// buffer with `len >= 256`.
unsafe fn write_bdl(bdl: *mut u32, phys: u64, len: usize) -> u16 {
    let first = (len / 2) & !127;
    let second = len - first;
    // SAFETY: caller's contract; two 16-byte entries fit in the page.
    unsafe {
        for (index, (address, bytes)) in [(phys, first), (phys + first as u64, second)]
            .into_iter()
            .enumerate()
        {
            let entry = bdl.add(index * 4);
            entry.write_volatile(address as u32);
            entry.add(1).write_volatile((address >> 32) as u32);
            entry.add(2).write_volatile(bytes as u32);
            entry.add(3).write_volatile(u32::from(index == 1)); // interrupt on completion
        }
    }
    1
}

/// Program output stream 0 with a two-entry BDL over `len` bytes of the speech
/// buffer at the speech format, and start it running. The DAC's converter format
/// is set to `SPEECH_FORMAT` once, in [`configure_speech`].
fn start_speech_stream(stream_base: u64, len: usize) {
    let bdl_phys = core::ptr::addr_of!(BDL) as u64;
    let speech_phys = core::ptr::addr_of!(SPEECH) as u64;
    let bdl = core::ptr::addr_of_mut!(BDL) as *mut u32;
    // SAFETY: BDL is the identity-mapped descriptor page; SPEECH is the DMA buffer and the
    // callers stage at least TAIL_SILENCE_BYTES.
    let lvi = unsafe { write_bdl(bdl, speech_phys, len) };
    // SAFETY: stream_base is inside the identity-mapped BAR0 register file.
    unsafe {
        mmio_write8(stream_base, SD_CTL, SDCTL_SRST);
        let mut budget = 1_000_000u32;
        while mmio_read8(stream_base, SD_CTL) & SDCTL_SRST == 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }
        mmio_write8(stream_base, SD_CTL, 0);
        let mut budget = 1_000_000u32;
        while mmio_read8(stream_base, SD_CTL) & SDCTL_SRST != 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }
        mmio_write32(stream_base, SD_CBL, len as u32);
        mmio_write16(stream_base, SD_LVI, lvi);
        mmio_write16(stream_base, SD_FMT, SPEECH_FORMAT);
        mmio_write32(stream_base, SD_BDPL, bdl_phys as u32);
        mmio_write32(stream_base, SD_BDPU, (bdl_phys >> 32) as u32);
        mmio_write8(stream_base, SD_CTL + 2, STREAM_TAG << 4);
        mmio_write8(
            stream_base,
            SD_CTL,
            mmio_read8(stream_base, SD_CTL) | SDCTL_RUN,
        );
    }
}

/// Clear the RUN bit on output stream 0.
fn stop_output_stream(stream_base: u64) {
    // SAFETY: clearing RUN on our own stream descriptor.
    unsafe {
        mmio_write8(
            stream_base,
            SD_CTL,
            mmio_read8(stream_base, SD_CTL) & !SDCTL_RUN,
        );
    }
}

/// The retained output stream base, or `None` until speech is set up.
fn speech_stream_base() -> Option<u64> {
    if !SPEECH_READY.load(Ordering::Acquire) {
        return None;
    }
    // SAFETY: single-core; the engine is stored before the ready flag is set.
    unsafe { (*SPEECH_ENGINE.0.get()).as_ref().map(|e| e.stream_base) }
}

/// Set the codec's DAC converter format to the speech format and retain output
/// stream 0 so the menu can play clips. Called at the end of [`prove`], once the
/// tone proof has found and configured the output path.
fn configure_speech(controller: &mut Controller, path: &OutputPath) -> bool {
    if controller
        .command16(path.dac, VERB4_SET_FORMAT, SPEECH_FORMAT)
        .is_err()
    {
        debug_write("AW_HDA_SPEECH_UNAVAILABLE reason=set_format\n");
        return false;
    }
    // SAFETY: single-core bring-up on the BSP.
    unsafe {
        *SPEECH_ENGINE.0.get() = Some(SpeechEngine {
            stream_base: controller.output_stream_base(),
        });
    }
    SPEECH_READY.store(true, Ordering::Release);
    debug_write("AW_HDA_SPEECH_READY\n");
    true
}

/// Speak one PCM clip through the codec, blocking until it has played once (or a
/// bounded budget elapses), then stopping the stream so the cyclic buffer does
/// not loop it. A no-op when the machine has no HDA output. Used by the menu.
pub fn speak(clip: &[u8]) {
    speak_pcm(clip, 24_000);
}

/// Speak mono 16-bit PCM at `rate` Hz (resampled to the 48 kHz stereo output).
pub fn speak_pcm(pcm: &[u8], rate: u32) {
    let Some(stream) = speech_stream_base() else {
        return;
    };
    let len = stage_pcm(pcm, rate);
    if len == 0 {
        return;
    }
    start_speech_stream(stream, len + TAIL_SILENCE_BYTES);
    // One pass: the link position climbs to the buffer end, then the cyclic
    // stream wraps back to zero - stop at the first end-or-wrap so the clip plays
    // exactly once rather than repeating.
    let target = len as u32;
    let mut prev = 0u32;
    let mut budget = 400_000_000u32;
    loop {
        // SAFETY: reading the stream's LPIB register is side-effect-free.
        let pos = unsafe { mmio_read32(stream, SD_LPIB) };
        if pos + 128 >= target || (prev != 0 && pos < prev) {
            break;
        }
        prev = pos;
        budget -= 1;
        if budget == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    stop_output_stream(stream);
}

/// Prove spoken output: play one clip and require the link position to advance,
/// which only happens when the controller is fetching PCM from memory and
/// clocking it to the codec - the same evidence the tone proof uses, now for real
/// speech. Prints an explicit marker and returns when speech never came up.
pub fn prove_speech(clip: &[u8]) {
    debug_write("AW_HDA_SPEECH_BEGIN\n");
    let Some(stream) = speech_stream_base() else {
        debug_write("AW_HDA_SPEECH_UNAVAILABLE reason=not_ready\n");
        return;
    };
    let len = stage_clip(clip);
    if len == 0 {
        debug_write("AW_HDA_SPEECH_FAIL reason=empty_clip\n");
        return;
    }
    start_speech_stream(stream, len + TAIL_SILENCE_BYTES);
    let mut moved = 0u32;
    let mut budget = 50_000_000u32;
    while budget > 0 {
        // SAFETY: reading LPIB is side-effect-free.
        moved = unsafe { mmio_read32(stream, SD_LPIB) };
        if moved > 0 {
            break;
        }
        budget -= 1;
        core::hint::spin_loop();
    }
    stop_output_stream(stream);
    if moved == 0 {
        debug_write("AW_HDA_SPEECH_FAIL reason=no_dma_progress\n");
        return;
    }
    debug_write("AW_HDA_SPEECH_DMA_ADVANCED position=");
    debug_write_u64(u64::from(moved));
    debug_write("\n");
    debug_write("AW_HDA_SPEECH_PROOF_OK\n");
    crate::firmware_runtime::pass(aw_generation::RuntimeHealthCheck::Speech);
}

/// Play a tone through the codec and prove the audio DMA runs: after the stream
/// starts, the link position (SD_LPIB) must advance past zero, which only happens
/// when the controller is fetching PCM from memory and clocking it to the codec.
/// Returns the discovered output path on success, so the caller can reuse it to
/// set up spoken output without walking the codec again.
fn prove_playback(controller: &mut Controller) -> Option<OutputPath> {
    let path = match find_output(controller) {
        Ok(path) => path,
        Err(reason) => {
            debug_write("AW_HDA_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
            return None;
        }
    };
    debug_write("AW_HDA_OUTPUT dac=");
    debug_write_u64(u64::from(path.dac));
    debug_write(" pin=");
    debug_write_u64(u64::from(path.pin));
    debug_write("\n");

    if let Err(reason) = configure_codec(controller, &path) {
        debug_write("AW_HDA_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return None;
    }

    fill_tone();
    start_stream(controller);
    debug_write("AW_HDA_STREAM_RUN\n");

    // Poll the link position: it must move as the controller streams the buffer.
    let stream = controller.output_stream_base();
    let mut moved = 0u32;
    let mut budget = 50_000_000u32;
    while budget > 0 {
        // SAFETY: reading the stream's LPIB register is side-effect-free.
        moved = unsafe { mmio_read32(stream, SD_LPIB) };
        if moved > 0 {
            break;
        }
        budget -= 1;
        core::hint::spin_loop();
    }

    if moved == 0 {
        debug_write("AW_HDA_FAIL reason=no_dma_progress\n");
        return None;
    }
    debug_write("AW_HDA_DMA_ADVANCED position=");
    debug_write_u64(u64::from(moved));
    debug_write("\n");

    // Stop the stream; the proof is made.
    // SAFETY: clearing RUN on our own stream descriptor.
    unsafe {
        mmio_write8(stream, SD_CTL, mmio_read8(stream, SD_CTL) & !SDCTL_RUN);
    }
    debug_write("AW_HDA_PLAYBACK_PROOF_OK\n");
    Some(path)
}

/// Prove the HDA audio path: find the controller, bring it out of reset, stand up
/// the CORB/RIRB rings, find the codec, and read the codec's vendor/device id back
/// over the ring - real data the codec produced, proving the command/response path
/// end to end. Prints `AW_HDA_UNAVAILABLE` and returns when no controller is
/// present, so it is safe on every boot configuration.
pub fn prove() {
    debug_write("AW_HDA_BEGIN\n");

    let Some(mut controller) = init() else {
        return;
    };

    let vendor = match controller.get_parameter(0, PARAM_VENDOR_ID) {
        Ok(vendor) => vendor,
        Err(reason) => {
            debug_write("AW_HDA_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };
    if vendor == 0 || vendor == 0xffff_ffff {
        debug_write("AW_HDA_FAIL reason=bad_vendor\n");
        return;
    }
    debug_write("AW_HDA_CODEC_ID vendor_device=");
    debug_write_hex_u64(u64::from(vendor));
    debug_write("\n");

    debug_write("AW_HDA_PROOF_OK\n");
    crate::firmware_runtime::pass(aw_generation::RuntimeHealthCheck::Audio);

    // With the command path proved, play a tone and prove the audio DMA runs,
    // then reuse the same output path to set the codec up for spoken output so the
    // accessible menu has a voice.
    if let Some(path) = prove_playback(&mut controller) {
        configure_speech(&mut controller, &path);
    }
}
