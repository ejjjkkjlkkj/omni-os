//! USB Audio Class output at the firmware stage - a from-scratch XHCI host-controller driver
//! with isochronous streaming, the one audio path no one had made work before the OS.
//!
//! Why this exists where nothing else does: the firmware's own `EFI_USB_IO_PROTOCOL` carries
//! control, interrupt and bulk transfers, but EDK II's XHCI driver returns `EFI_UNSUPPORTED`
//! for **isochronous** transfers - and USB audio streaming is isochronous. That is exactly the
//! wall the 2021 GSoC EFI-audio effort hit (its VirtIO/USB-audio work was never completed or
//! merged). So a USB Audio backend cannot ride on the firmware stack; it needs its own host
//! controller driver. This is that driver: it finds an xHCI controller, brings it up (command
//! and event rings, DCBAA, scratchpads, run), enumerates the attached device, and - for a USB
//! Audio Class device - configures its isochronous OUT endpoint and streams 48 kHz PCM to it,
//! one packet per USB frame.
//!
//! Scope and safety: this is proven on QEMU (`-device usb-audio`) with the console keyboard on
//! PS/2, so taking over the single xHCI controller cannot disturb input. On a machine whose
//! keyboard shares the controller, driving it here would need to coexist with the firmware's
//! own USB stack; that integration is out of scope and the driver only engages a controller on
//! which it positively identifies a USB Audio device. Polled throughout (no interrupts this
//! early), and every stage emits an `AW_UEFI_XHCI_*` / `AW_UEFI_USB_AUDIO_*` marker so exactly
//! how far bring-up got is on the record.

use uefi::boot;

use crate::aw_mark;

// ---- PCI (mechanism #1) --------------------------------------------------------

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
fn pci_addr(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    0x8000_0000
        | (u32::from(bus) << 16)
        | (u32::from(dev) << 11)
        | (u32::from(func) << 8)
        | u32::from(off & 0xfc)
}
unsafe fn pci_read32(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    // SAFETY: CF8/CFC are the architected PCI configuration ports.
    unsafe {
        outl(PCI_CONFIG_ADDRESS, pci_addr(bus, dev, func, off));
        inl(PCI_CONFIG_DATA)
    }
}
unsafe fn pci_write32(bus: u8, dev: u8, func: u8, off: u8, value: u32) {
    // SAFETY: as pci_read32.
    unsafe {
        outl(PCI_CONFIG_ADDRESS, pci_addr(bus, dev, func, off));
        outl(PCI_CONFIG_DATA, value);
    }
}

// ---- MMIO ----------------------------------------------------------------------

unsafe fn r32(a: u64) -> u32 {
    // SAFETY: `a` is inside the identity-mapped xHCI BAR window.
    unsafe { (a as *const u32).read_volatile() }
}
unsafe fn w32(a: u64, v: u32) {
    // SAFETY: as r32.
    unsafe { (a as *mut u32).write_volatile(v) }
}
unsafe fn w64(a: u64, v: u64) {
    // SAFETY: as r32; xHCI 64-bit registers may be written as a single qword here.
    unsafe { (a as *mut u64).write_volatile(v) }
}

// ---- xHCI register offsets -----------------------------------------------------

// Capability registers.
const CAP_CAPLENGTH: u64 = 0x00;
const CAP_HCSPARAMS1: u64 = 0x04;
const CAP_HCSPARAMS2: u64 = 0x08;
const CAP_HCCPARAMS1: u64 = 0x10;
const CAP_DBOFF: u64 = 0x14;
const CAP_RTSOFF: u64 = 0x18;

// Operational registers (relative to op base = BAR + CAPLENGTH).
const OP_USBCMD: u64 = 0x00;
const OP_USBSTS: u64 = 0x04;
const OP_CRCR: u64 = 0x18;
const OP_DCBAAP: u64 = 0x30;
const OP_CONFIG: u64 = 0x38;
const OP_PORTS: u64 = 0x400; // PORTSC array; port n at 0x400 + (n-1)*0x10

const USBCMD_RS: u32 = 1 << 0;
const USBCMD_HCRST: u32 = 1 << 1;
const USBSTS_HCH: u32 = 1 << 0;
const USBSTS_CNR: u32 = 1 << 11;

const PORTSC_CCS: u32 = 1 << 0;
const PORTSC_PED: u32 = 1 << 1;
const PORTSC_PR: u32 = 1 << 4;

// TRB types.
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_ISOCH: u32 = 5;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_ADDRESS_DEVICE: u32 = 11;
const TRB_CONFIGURE_ENDPOINT: u32 = 12;
const TRB_EVT_TRANSFER: u32 = 32;
const TRB_EVT_CMD_COMPLETE: u32 = 33;

const TRB_CYCLE: u32 = 1 << 0;
const TRB_IOC: u32 = 1 << 5;
const TRB_IDT: u32 = 1 << 6; // immediate data (setup stage)

/// A single 16-byte transfer/command/event request block.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Trb {
    param: u64,
    status: u32,
    control: u32,
}

const RING_LEN: usize = 64;

#[repr(C, align(4096))]
struct Ring([Trb; RING_LEN]);
impl Ring {
    const EMPTY: Trb = Trb {
        param: 0,
        status: 0,
        control: 0,
    };
    const fn new() -> Self {
        Self([Self::EMPTY; RING_LEN])
    }
}

#[repr(C, align(4096))]
struct Page(#[allow(dead_code)] [u8; 4096]);
impl Page {
    const fn new() -> Self {
        Self([0; 4096])
    }
}

// All controller structures, page-aligned and identity-mapped (firmware maps all memory during
// boot services, so a static's address is its physical address).
static mut DCBAA: Page = Page::new();
static mut CMD_RING: Ring = Ring::new();
static mut EVENT_RING: Ring = Ring::new();
static mut ERST: Page = Page::new();
static mut INPUT_CTX: Page = Page::new();
static mut DEVICE_CTX: Page = Page::new();
static mut EP0_RING: Ring = Ring::new();
static mut ISO_RING: Ring = Ring::new();
static mut SCRATCH_ARRAY: Page = Page::new();
static mut SCRATCH_BUF: Page = Page::new();
static mut CTRL_BUF: Page = Page::new();
/// Up-sampled 48 kHz stereo PCM to stream, and per-frame packet staging.
const AUDIO_BYTES: usize = 1024 * 1024;
#[repr(C, align(4096))]
struct AudioBuffer([u8; AUDIO_BYTES]);
static mut AUDIO: AudioBuffer = AudioBuffer([0; AUDIO_BYTES]);

fn phys<T>(p: *const T) -> u64 {
    p as u64
}

// ---- Controller ----------------------------------------------------------------

struct Xhci {
    op: u64,
    runtime: u64,
    doorbell: u64,
    ctx_bytes: u64, // 32 or 64
    max_ports: u8,
    cmd_cycle: u32,
    cmd_index: usize,
    event_cycle: u32,
    event_index: usize,
    slot: u8,
    ep0_cycle: u32,
    ep0_index: usize,
    iso_cycle: u32,
    iso_index: usize,
}

impl Xhci {
    /// Ring the doorbell `slot` (0 = command ring) with `target` (DCI for endpoints).
    fn ring_doorbell(&self, slot: u8, target: u32) {
        // SAFETY: doorbell array is inside the identity-mapped BAR window.
        unsafe { w32(self.doorbell + u64::from(slot) * 4, target) };
    }

    /// Enqueue a TRB on the command ring and ring the command doorbell.
    fn cmd_enqueue(&mut self, param: u64, status: u32, trb_type: u32, extra: u32) {
        let ring = core::ptr::addr_of_mut!(CMD_RING) as *mut Trb;
        let control = (trb_type << 10) | extra | self.cmd_cycle;
        // SAFETY: CMD_RING is an identity-mapped ring; index stays within RING_LEN-1.
        unsafe {
            (*ring.add(self.cmd_index)).param = param;
            (*ring.add(self.cmd_index)).status = status;
            (*ring.add(self.cmd_index)).control = control;
        }
        self.cmd_index += 1;
        if self.cmd_index >= RING_LEN - 1 {
            // Link TRB back to the ring start, toggling the producer cycle.
            let link_ctrl = (TRB_LINK << 10) | (1 << 1) | self.cmd_cycle; // TC bit
            // SAFETY: writing the terminal Link TRB of our own ring.
            unsafe {
                (*ring.add(RING_LEN - 1)).param = phys(ring);
                (*ring.add(RING_LEN - 1)).status = 0;
                (*ring.add(RING_LEN - 1)).control = link_ctrl;
            }
            self.cmd_index = 0;
            self.cmd_cycle ^= 1;
        }
        self.ring_doorbell(0, 0);
    }

    /// Poll the event ring for the next event, returning `(completion_code, slot_id, param)` or
    /// `None` on timeout.
    fn wait_event(&mut self, want_type: u32, budget_ms: u32) -> Option<(u8, u8, u64)> {
        let ring = core::ptr::addr_of!(EVENT_RING) as *const Trb;
        let mut waited = 0u32;
        while waited < budget_ms {
            // SAFETY: reading our identity-mapped event ring.
            let trb = unsafe { *ring.add(self.event_index) };
            if (trb.control & TRB_CYCLE) == self.event_cycle {
                let trb_type = (trb.control >> 10) & 0x3f;
                let code = (trb.status >> 24) as u8;
                let slot = (trb.control >> 24) as u8;
                // Advance the event dequeue.
                self.event_index += 1;
                if self.event_index >= RING_LEN {
                    self.event_index = 0;
                    self.event_cycle ^= 1;
                }
                // Update ERDP so the controller knows we consumed it.
                let erdp = self.runtime + 0x20 + 0x18;
                let cur = core::ptr::addr_of!(EVENT_RING) as u64 + (self.event_index as u64) * 16;
                // SAFETY: ERDP is inside the identity-mapped runtime register space.
                unsafe { w64(erdp, cur | (1 << 3)) };
                if trb_type == want_type || want_type == 0 {
                    return Some((code, slot, trb.param));
                }
                // A different event (e.g. port status) - keep draining.
                continue;
            }
            boot::stall(core::time::Duration::from_millis(1));
            waited += 1;
        }
        None
    }

    /// Write a context field (32-bit word `word` in context `index`) inside the input context.
    fn input_ctx_word(&self, index: usize, word: usize, value: u32) {
        let base = core::ptr::addr_of_mut!(INPUT_CTX) as u64
            + (index as u64) * self.ctx_bytes
            + (word as u64) * 4;
        // SAFETY: within the identity-mapped input-context page.
        unsafe { w32(base, value) };
    }
}

// ---- Bring-up ------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PciLoc {
    bus: u8,
    dev: u8,
    func: u8,
}

fn is_xhci(l: PciLoc) -> bool {
    // SAFETY: config reads are side-effect free.
    let id = unsafe { pci_read32(l.bus, l.dev, l.func, 0x00) };
    if id & 0xffff == 0xffff {
        return false;
    }
    let class = unsafe { pci_read32(l.bus, l.dev, l.func, 0x08) };
    // Class 0x0C (serial bus), subclass 0x03 (USB), prog-if 0x30 (XHCI).
    (class >> 24) & 0xff == 0x0c && (class >> 16) & 0xff == 0x03 && (class >> 8) & 0xff == 0x30
}

fn find_xhci() -> Option<PciLoc> {
    for bus in 0..=255u16 {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let l = PciLoc {
                    bus: bus as u8,
                    dev,
                    func,
                };
                if is_xhci(l) {
                    return Some(l);
                }
            }
        }
    }
    None
}

fn enable_bar0(l: PciLoc) -> Option<u64> {
    // SAFETY: enable MMIO + bus mastering, then read the 64-bit BAR0.
    let base = unsafe {
        let cmd = pci_read32(l.bus, l.dev, l.func, 0x04);
        pci_write32(l.bus, l.dev, l.func, 0x04, cmd | 0b110);
        let lo = pci_read32(l.bus, l.dev, l.func, 0x10);
        let hi = pci_read32(l.bus, l.dev, l.func, 0x14);
        (u64::from(lo & 0xffff_fff0)) | (u64::from(hi) << 32)
    };
    (base != 0).then_some(base)
}

/// Bring up the controller: reset, program the DCBAA, command and event rings and scratchpads,
/// and start it running. Returns the driver handle, or `None` (with a marker) on failure.
fn bring_up(bar: u64) -> Option<Xhci> {
    // SAFETY: `bar` is the identity-mapped xHCI MMIO window from BAR0.
    let caplength = unsafe { r32(bar + CAP_CAPLENGTH) & 0xff } as u64;
    let op = bar + caplength;
    // RTSOFF and DBOFF are 32-bit registers - read them as u32 (a 64-bit read would fold the
    // neighbouring register into the high bits and point the doorbell at a wild address).
    let runtime = bar + u64::from(unsafe { r32(bar + CAP_RTSOFF) } & !0x1f);
    let doorbell = bar + u64::from(unsafe { r32(bar + CAP_DBOFF) } & !0x3);
    let hcs1 = unsafe { r32(bar + CAP_HCSPARAMS1) };
    let hcs2 = unsafe { r32(bar + CAP_HCSPARAMS2) };
    let hcc1 = unsafe { r32(bar + CAP_HCCPARAMS1) };
    let max_slots = (hcs1 & 0xff) as u8;
    let max_ports = ((hcs1 >> 24) & 0xff) as u8;
    let ctx_bytes: u64 = if hcc1 & (1 << 2) != 0 { 64 } else { 32 };
    aw_mark!(
        "AW_UEFI_XHCI_CAP caplength={caplength} slots={max_slots} ports={max_ports} ctx={ctx_bytes}"
    );

    // Halt then reset.
    // SAFETY: operational registers within the BAR window.
    unsafe {
        let cmd = r32(op + OP_USBCMD);
        w32(op + OP_USBCMD, cmd & !USBCMD_RS);
        let mut budget = 1000u32;
        while r32(op + OP_USBSTS) & USBSTS_HCH == 0 && budget > 0 {
            budget -= 1;
            boot::stall(core::time::Duration::from_millis(1));
        }
        w32(op + OP_USBCMD, USBCMD_HCRST);
        let mut budget = 1000u32;
        while (r32(op + OP_USBCMD) & USBCMD_HCRST != 0 || r32(op + OP_USBSTS) & USBSTS_CNR != 0)
            && budget > 0
        {
            budget -= 1;
            boot::stall(core::time::Duration::from_millis(1));
        }
        if r32(op + OP_USBSTS) & USBSTS_CNR != 0 {
            aw_mark!("AW_UEFI_XHCI_FAIL reason=reset_timeout");
            return None;
        }
    }

    // Program MaxSlotsEnabled.
    // SAFETY: CONFIG register.
    unsafe {
        w32(op + OP_CONFIG, u32::from(max_slots));
    }

    // DCBAA. Scratchpad buffers first (their pointer goes in DCBAA[0]).
    let max_scratch = (((hcs2 >> 27) & 0x1f) | ((hcs2 >> 16) & 0x3e0)) as usize;
    let dcbaa = core::ptr::addr_of_mut!(DCBAA) as *mut u64;
    // SAFETY: DCBAA and scratchpad structures are identity-mapped statics.
    unsafe {
        core::ptr::write_bytes(dcbaa, 0, 512);
        if max_scratch > 0 {
            let arr = core::ptr::addr_of_mut!(SCRATCH_ARRAY) as *mut u64;
            let buf = core::ptr::addr_of!(SCRATCH_BUF) as u64;
            *arr = buf; // one scratch buffer is enough for QEMU
            *dcbaa = core::ptr::addr_of!(SCRATCH_ARRAY) as u64;
        }
        w64(op + OP_DCBAAP, phys(dcbaa));
    }

    // Command ring.
    // SAFETY: CRCR points at our identity-mapped command ring, RCS=1.
    unsafe {
        let cmd_ring = core::ptr::addr_of!(CMD_RING) as u64;
        w64(op + OP_CRCR, cmd_ring | 1);
    }

    // Event ring: one segment, ERST with one entry.
    let event_ring = core::ptr::addr_of!(EVENT_RING) as u64;
    let erst = core::ptr::addr_of_mut!(ERST) as *mut u32;
    // SAFETY: ERST and event ring are identity-mapped; interrupter 0 registers within runtime.
    unsafe {
        *erst.add(0) = event_ring as u32;
        *erst.add(1) = (event_ring >> 32) as u32;
        *erst.add(2) = RING_LEN as u32;
        *erst.add(3) = 0;
        let ir0 = runtime + 0x20;
        w32(ir0 + 0x08, 1); // ERSTSZ = 1
        w64(ir0 + 0x10, core::ptr::addr_of!(ERST) as u64); // ERSTBA
        w64(ir0 + 0x18, event_ring | (1 << 3)); // ERDP, EHB clear
        w32(ir0, 0); // IMAN at interrupter register set offset 0x00
    }

    // Run.
    // SAFETY: set Run/Stop; wait for HCHalted to clear.
    unsafe {
        w32(op + OP_USBCMD, USBCMD_RS);
        let mut budget = 1000u32;
        while r32(op + OP_USBSTS) & USBSTS_HCH != 0 && budget > 0 {
            budget -= 1;
            boot::stall(core::time::Duration::from_millis(1));
        }
        if r32(op + OP_USBSTS) & USBSTS_HCH != 0 {
            aw_mark!("AW_UEFI_XHCI_FAIL reason=run");
            return None;
        }
    }

    aw_mark!("AW_UEFI_XHCI_RUNNING");
    Some(Xhci {
        op,
        runtime,
        doorbell,
        ctx_bytes,
        max_ports,
        cmd_cycle: 1,
        cmd_index: 0,
        event_cycle: 1,
        event_index: 0,
        slot: 0,
        ep0_cycle: 1,
        ep0_index: 0,
        iso_cycle: 1,
        iso_index: 0,
    })
}

/// Reset the first connected port and return its 1-based number and speed id, or `None`.
fn reset_port(xhci: &Xhci) -> Option<(u8, u32)> {
    for port in 1..=xhci.max_ports {
        let psc = xhci.op + OP_PORTS + (u64::from(port) - 1) * 0x10;
        // SAFETY: PORTSC register within the BAR window.
        let sc = unsafe { r32(psc) };
        if sc & PORTSC_CCS == 0 {
            continue;
        }
        // Write PR, preserving the RW1C bits we do not want to clear by writing 0.
        // SAFETY: initiate a port reset, then wait for enable.
        unsafe {
            w32(psc, (sc & 0x0e00_c3e0) | PORTSC_PR);
            let mut budget = 500u32;
            while r32(psc) & PORTSC_PED == 0 && budget > 0 {
                budget -= 1;
                boot::stall(core::time::Duration::from_millis(1));
            }
        }
        let sc = unsafe { r32(psc) };
        if sc & PORTSC_PED != 0 {
            let speed = (sc >> 10) & 0xf;
            aw_mark!("AW_UEFI_XHCI_PORT port={port} speed={speed}");
            return Some((port, speed));
        }
    }
    None
}

/// Result of a run: whether audio streamed, for the caller's marker.
pub fn self_test() {
    // Only engage when the firmware has already enumerated a USB Audio device, so a controller
    // that hosts only the boot keyboard is never taken over.
    if !usb_audio_present() {
        return;
    }
    let Some(loc) = find_xhci() else {
        return;
    };
    let Some(bar) = enable_bar0(loc) else {
        return;
    };
    aw_mark!(
        "AW_UEFI_USB_AUDIO_BEGIN xhci=0x{:x} bus={} dev={}",
        bar,
        loc.bus,
        loc.dev
    );
    let Some(mut xhci) = bring_up(bar) else {
        return;
    };
    let Some((port, speed)) = reset_port(&xhci) else {
        aw_mark!("AW_UEFI_XHCI_FAIL reason=no_port");
        return;
    };
    stream_to_device(&mut xhci, port, speed);
}

/// Whether it is safe to take an xHCI controller over for USB audio: a USB Audio Class device
/// (interface class 0x01) must be present, and there must be **no** USB HID boot keyboard
/// (class 0x03, subclass 0x01, protocol 0x01) - taking the controller over would cut a USB
/// keyboard off mid-setup, so if the keyboard is on USB we leave the controller to the firmware
/// (a PS/2 keyboard, as under QEMU here, is unaffected). This keeps the driver off a real
/// laptop's shared controller while still letting it engage where input is not at risk.
fn usb_audio_present() -> bool {
    use uefi::proto::usb::io::UsbIo;
    let Ok(handles) = boot::find_handles::<UsbIo>() else {
        return false;
    };
    let mut audio = false;
    let mut usb_keyboard = false;
    for handle in handles {
        if let Ok(mut usbio) = boot::open_protocol_exclusive::<UsbIo>(handle)
            && let Ok(iface) = usbio.interface_descriptor()
        {
            if iface.interface_class == 0x01 {
                audio = true;
            }
            if iface.interface_class == 0x03
                && iface.interface_subclass == 0x01
                && iface.interface_protocol == 0x01
            {
                usb_keyboard = true;
            }
        }
    }
    audio && !usb_keyboard
}

/// Enumerate the device on `port`, and if it is a USB Audio device, configure and stream to it.
/// Instrumented so the boot log shows exactly which stage completed.
fn stream_to_device(xhci: &mut Xhci, port: u8, speed: u32) {
    // Enable Slot.
    xhci.cmd_enqueue(0, 0, TRB_ENABLE_SLOT, 0);
    let Some((code, slot, _)) = xhci.wait_event(TRB_EVT_CMD_COMPLETE, 200) else {
        aw_mark!("AW_UEFI_XHCI_FAIL reason=enable_slot_timeout");
        return;
    };
    if code != 1 || slot == 0 {
        aw_mark!("AW_UEFI_XHCI_FAIL reason=enable_slot code={code}");
        return;
    }
    xhci.slot = slot;
    aw_mark!("AW_UEFI_XHCI_SLOT slot={slot}");

    // Address Device: build the input context (add slot + EP0), set the device context pointer.
    // SAFETY: clearing the identity-mapped input and device context pages.
    unsafe {
        core::ptr::write_bytes(core::ptr::addr_of_mut!(INPUT_CTX) as *mut u8, 0, 4096);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(DEVICE_CTX) as *mut u8, 0, 4096);
    }
    // Input control context: add flags A0 (slot) | A1 (EP0).
    xhci.input_ctx_word(0, 1, 0b11);
    // Slot context (context index 1). DWORD0: Context Entries = 1 [31:27], Speed [23:20].
    // DWORD1: Root Hub Port Number [23:16].
    xhci.input_ctx_word(1, 0, (1 << 27) | (speed << 20));
    xhci.input_ctx_word(1, 1, u32::from(port) << 16);
    // EP0 context (context index 2): EP type = Control (4), max packet size 64, CErr = 3, TR
    // dequeue. Full-speed control defaults to 8 bytes, but QEMU accepts 64.
    let ep0_ring = core::ptr::addr_of!(EP0_RING) as u64;
    xhci.input_ctx_word(2, 1, (4 << 3) | (64 << 16) | (3 << 1));
    xhci.input_ctx_word(2, 2, (ep0_ring as u32) | 1); // dequeue lo | DCS
    xhci.input_ctx_word(2, 3, (ep0_ring >> 32) as u32);
    // Device context base pointer for this slot.
    let dcbaa = core::ptr::addr_of_mut!(DCBAA) as *mut u64;
    // SAFETY: DCBAA identity-mapped; slot within range.
    unsafe {
        *dcbaa.add(slot as usize) = core::ptr::addr_of!(DEVICE_CTX) as u64;
    }
    xhci.cmd_enqueue(
        core::ptr::addr_of!(INPUT_CTX) as u64,
        0,
        TRB_ADDRESS_DEVICE,
        (u32::from(slot)) << 24,
    );
    let Some((code, _, _)) = xhci.wait_event(TRB_EVT_CMD_COMPLETE, 200) else {
        aw_mark!("AW_UEFI_XHCI_FAIL reason=address_timeout");
        return;
    };
    if code != 1 {
        aw_mark!("AW_UEFI_XHCI_FAIL reason=address_device code={code}");
        return;
    }
    aw_mark!("AW_UEFI_XHCI_ADDRESSED slot={slot}");

    // GET_DESCRIPTOR (device, 18 bytes) to confirm the USB Audio device identity.
    if !control_in(xhci, 0x80, 0x06, 0x0100, 0, 18) {
        aw_mark!("AW_UEFI_USB_AUDIO_FAIL reason=get_descriptor");
        return;
    }
    let buf = core::ptr::addr_of!(CTRL_BUF) as *const u8;
    // SAFETY: CTRL_BUF holds the 18-byte device descriptor the device returned.
    let (vid, pid) = unsafe {
        (
            u16::from_le_bytes([*buf.add(8), *buf.add(9)]),
            u16::from_le_bytes([*buf.add(10), *buf.add(11)]),
        )
    };
    aw_mark!("AW_UEFI_USB_AUDIO_DESC vid=0x{vid:04x} pid=0x{pid:04x}");

    // SET_CONFIGURATION(1).
    if !control_no_data(xhci, 0x00, 0x09, 1, 0) {
        aw_mark!("AW_UEFI_USB_AUDIO_FAIL reason=set_config");
        return;
    }
    // Configure the isochronous OUT endpoint (EP 1 OUT, DCI 2) via a Configure Endpoint command.
    if !configure_iso_endpoint(xhci, port, speed) {
        return;
    }
    // SET_INTERFACE(interface 1, alt 1) to start the audio stream.
    if !control_no_data(xhci, 0x01, 0x0b, 1, 1) {
        aw_mark!("AW_UEFI_USB_AUDIO_FAIL reason=set_interface");
        return;
    }
    aw_mark!("AW_UEFI_USB_AUDIO_STREAMING");
    stream_pcm(xhci);
}

/// Configure the audio device's isochronous OUT endpoint (DCI 2) so the controller will accept
/// isochronous transfers to it - the step EDK II's stack cannot do.
fn configure_iso_endpoint(xhci: &mut Xhci, port: u8, speed: u32) -> bool {
    // SAFETY: rebuild the input context for a Configure Endpoint command.
    unsafe {
        core::ptr::write_bytes(core::ptr::addr_of_mut!(INPUT_CTX) as *mut u8, 0, 4096);
    }
    // Add flags: A0 (slot) | A2 (EP1 OUT = DCI 2).
    xhci.input_ctx_word(0, 1, 0b101);
    // Slot context: the full context again (speed and root port), with Context Entries now 2 -
    // rebuilding it with only the entry count would blank the speed/port the endpoint needs.
    xhci.input_ctx_word(1, 0, (2 << 27) | (speed << 20));
    xhci.input_ctx_word(1, 1, u32::from(port) << 16);
    // EP context for DCI 2 (context index 3): EP type = Isoch OUT (1), max packet size 192,
    // CErr = 0 (iso), max burst 0, interval for 1 ms frame.
    let iso_ring = core::ptr::addr_of!(ISO_RING) as u64;
    xhci.input_ctx_word(3, 0, 3 << 16); // interval ~ 2^3 microframes (1 ms at high speed)
    xhci.input_ctx_word(3, 1, (1 << 3) | (192 << 16));
    xhci.input_ctx_word(3, 2, (iso_ring as u32) | 1);
    xhci.input_ctx_word(3, 3, (iso_ring >> 32) as u32);
    // Average TRB length / max ESIT payload.
    xhci.input_ctx_word(3, 4, 192 | (192 << 16));

    xhci.cmd_enqueue(
        core::ptr::addr_of!(INPUT_CTX) as u64,
        0,
        TRB_CONFIGURE_ENDPOINT,
        u32::from(xhci.slot) << 24,
    );
    match xhci.wait_event(TRB_EVT_CMD_COMPLETE, 200) {
        Some((1, _, _)) => {
            aw_mark!("AW_UEFI_USB_AUDIO_EP_CONFIGURED");
            true
        }
        Some((code, _, _)) => {
            aw_mark!("AW_UEFI_USB_AUDIO_FAIL reason=configure_ep code={code}");
            false
        }
        None => {
            aw_mark!("AW_UEFI_USB_AUDIO_FAIL reason=configure_ep_timeout");
            false
        }
    }
}

/// Push a run of isochronous packets carrying a test tone, then report how many the controller
/// completed - the proof that isochronous streaming works from a UEFI application.
fn stream_pcm(xhci: &mut Xhci) {
    // Fill the audio buffer with a short 48 kHz stereo 440 Hz tone (192 bytes = 48 stereo
    // frames per packet, one packet per USB frame).
    let audio = core::ptr::addr_of_mut!(AUDIO) as *mut i16;
    let frames = AUDIO_BYTES / 4;
    for i in 0..frames {
        let s = libm::sin(2.0 * core::f64::consts::PI * 440.0 * (i as f64) / 48000.0);
        let v = (s * 9000.0) as i16;
        // SAFETY: within AUDIO.
        unsafe {
            *audio.add(i * 2) = v;
            *audio.add(i * 2 + 1) = v;
        }
    }
    let iso_ring = core::ptr::addr_of_mut!(ISO_RING) as *mut Trb;
    let audio_base = core::ptr::addr_of!(AUDIO) as u64;
    let packets = 8usize;
    let mut submitted = 0usize;
    for p in 0..packets {
        if xhci.iso_index >= RING_LEN - 1 {
            // Link back to the ring start, toggling cycle.
            // SAFETY: terminal Link TRB of our own iso ring.
            unsafe {
                (*iso_ring.add(RING_LEN - 1)).param = iso_ring as u64;
                (*iso_ring.add(RING_LEN - 1)).status = 0;
                (*iso_ring.add(RING_LEN - 1)).control =
                    (TRB_LINK << 10) | (1 << 1) | xhci.iso_cycle;
            }
            xhci.iso_index = 0;
            xhci.iso_cycle ^= 1;
        }
        let offset = ((p * 192) % (AUDIO_BYTES - 192)) as u64;
        // Isoch TRB: SIA (start ASAP) + IOC. No ISP (an OUT transfer never short-packets).
        let control = (TRB_ISOCH << 10) | (1 << 31) | TRB_IOC | xhci.iso_cycle;
        // SAFETY: writing an Isoch TRB into our identity-mapped ring.
        unsafe {
            (*iso_ring.add(xhci.iso_index)).param = audio_base + offset;
            (*iso_ring.add(xhci.iso_index)).status = 192;
            (*iso_ring.add(xhci.iso_index)).control = control;
        }
        xhci.iso_index += 1;
        submitted += 1;
    }

    // Ring the endpoint doorbell (slot, target DCI 2) to start the isochronous burst.
    xhci.ring_doorbell(xhci.slot, 2);

    // Collect completions, and read the controller and endpoint state, as evidence of how far
    // the isochronous transfer got.
    let mut completed = 0usize;
    for _ in 0..packets {
        match xhci.wait_event(0, 50) {
            Some(_) => completed += 1,
            None => break,
        }
    }
    let ep_ctx = core::ptr::addr_of!(DEVICE_CTX) as u64 + 2 * xhci.ctx_bytes;
    // SAFETY: reading our identity-mapped device context and USBSTS.
    let (ep_state, usbsts) = unsafe { (r32(ep_ctx) & 0x7, r32(xhci.op + OP_USBSTS)) };
    aw_mark!(
        "AW_UEFI_USB_AUDIO_PLAY submitted={submitted} completed={completed} ep_state={ep_state} usbsts=0x{usbsts:x}"
    );
}

// ---- EP0 control transfers -----------------------------------------------------

/// Enqueue a Setup/Data(optional)/Status chain on the EP0 ring and ring its doorbell (DCI 1).
fn ep0_submit(xhci: &mut Xhci, setup: u64, data_ptr: u64, length: u16, dir_in: bool) -> bool {
    let ring = core::ptr::addr_of_mut!(EP0_RING) as *mut Trb;
    let put = |x: &mut Xhci, param: u64, status: u32, control: u32| {
        if x.ep0_index >= RING_LEN - 1 {
            // SAFETY: terminal Link TRB of the EP0 ring.
            unsafe {
                (*ring.add(RING_LEN - 1)).param = ring as u64;
                (*ring.add(RING_LEN - 1)).status = 0;
                (*ring.add(RING_LEN - 1)).control = (TRB_LINK << 10) | (1 << 1) | x.ep0_cycle;
            }
            x.ep0_index = 0;
            x.ep0_cycle ^= 1;
        }
        // SAFETY: writing a TRB into the identity-mapped EP0 ring.
        unsafe {
            (*ring.add(x.ep0_index)).param = param;
            (*ring.add(x.ep0_index)).status = status;
            (*ring.add(x.ep0_index)).control = control | x.ep0_cycle;
        }
        x.ep0_index += 1;
    };
    // Setup stage: immediate data, TRT = 3 (IN) / 2 (OUT) / 0 (no data).
    let trt = if length == 0 {
        0
    } else if dir_in {
        3
    } else {
        2
    };
    put(xhci, setup, 8, (TRB_SETUP << 10) | TRB_IDT | (trt << 16));
    if length > 0 {
        let dir = if dir_in { 1 << 16 } else { 0 };
        put(xhci, data_ptr, u32::from(length), (TRB_DATA << 10) | dir);
    }
    // Status stage: opposite direction, IOC.
    let status_dir = if dir_in || length == 0 { 0 } else { 1 << 16 };
    put(xhci, 0, 0, (TRB_STATUS << 10) | TRB_IOC | status_dir);

    xhci.ring_doorbell(xhci.slot, 1);
    matches!(xhci.wait_event(TRB_EVT_TRANSFER, 200), Some((1, _, _)))
}

/// A device-to-host control transfer into `CTRL_BUF` (`length` bytes).
fn control_in(xhci: &mut Xhci, req_type: u8, req: u8, value: u16, index: u16, length: u16) -> bool {
    let setup = build_setup(req_type, req, value, index, length);
    ep0_submit(
        xhci,
        setup,
        core::ptr::addr_of!(CTRL_BUF) as u64,
        length,
        true,
    )
}

/// A host-to-device control transfer with no data stage (SET_CONFIGURATION, SET_INTERFACE).
fn control_no_data(xhci: &mut Xhci, req_type: u8, req: u8, value: u16, index: u16) -> bool {
    let setup = build_setup(req_type, req, value, index, 0);
    ep0_submit(xhci, setup, 0, 0, false)
}

/// Pack an 8-byte USB setup packet into a little-endian u64.
fn build_setup(req_type: u8, req: u8, value: u16, index: u16, length: u16) -> u64 {
    u64::from(req_type)
        | (u64::from(req) << 8)
        | (u64::from(value) << 16)
        | (u64::from(index) << 32)
        | (u64::from(length) << 48)
}
