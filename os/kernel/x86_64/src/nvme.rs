//! Minimal NVMe driver: bring up the controller and read its IDENTIFY data
//! (dossier section 11.2 "NVMe/AHCI", roadmap Phase 3 "NVMe controller
//! initialization and identify").
//!
//! AHCI proved a real SATA controller; this proves the interface modern PCs boot
//! their SSDs through. NVMe is memory-mapped, not port-mapped: everything happens
//! through a BAR0 register block and a pair of DMA queues in RAM. The proof sets
//! up the admin submission/completion queues, enables the controller, issues one
//! IDENTIFY CONTROLLER command, and reads the model number the controller wrote
//! back by DMA - real content the device produced, not a status bit.
//!
//! Only what a single admin command needs is implemented: the admin queue pair,
//! one command, polled completion by the CQ phase tag, no interrupts, no I/O
//! queues, no namespaces. Every structure the controller touches by DMA lives in
//! a page-aligned `static` the identity map covers 1:1, so its virtual address is
//! also the physical address handed to the controller. IDENTIFY is read-only and
//! safe on any machine, so this runs on the normal boot path.

use aw_x86_paging::PageTableFlags;

use crate::page_mapper::map_page;
use crate::{debug_write, debug_write_hex_u64, debug_write_u64};

// ---- x86 port I/O for PCI mechanism #1 (CF8/CFC) ------------------------

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

// ---- MMIO on the controller's BAR0 register block -----------------------

unsafe fn mmio_read32(base: u64, offset: u64) -> u32 {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 MMIO window.
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

unsafe fn mmio_write32(base: u64, offset: u64, value: u32) {
    // SAFETY: `base+offset` is inside the identity-mapped BAR0 MMIO window.
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
}

// ---- Controller register offsets (NVMe base spec) -----------------------

const REG_CAP: u64 = 0x00; // capabilities (64-bit)
const REG_CC: u64 = 0x14; // controller configuration
const REG_CSTS: u64 = 0x1c; // controller status
const REG_AQA: u64 = 0x24; // admin queue attributes
const REG_ASQ: u64 = 0x28; // admin submission queue base (64-bit)
const REG_ACQ: u64 = 0x30; // admin completion queue base (64-bit)
const DOORBELL_BASE: u64 = 0x1000;

const CC_EN: u32 = 1 << 0;
const CSTS_RDY: u32 = 1 << 0;
const CSTS_CFS: u32 = 1 << 1; // controller fatal status

/// CC with the NVM command set, 4 KiB pages, 64-byte SQ / 16-byte CQ entries,
/// and the enable bit: IOSQES=6, IOCQES=4, CSS=0, MPS=0, EN=1.
const CC_ENABLE: u32 = (4 << 20) | (6 << 16) | CC_EN;

const OPCODE_IDENTIFY: u8 = 0x06;
const IDENTIFY_CNS_CONTROLLER: u32 = 1;

/// Admin queue depth (entries). Small: the proof issues one command.
const QUEUE_DEPTH: u32 = 64;

/// The bring-up map identity-covers the low 4 GiB; a BAR at or above this must be
/// mapped into the page tables before it can be touched.
const IDENTITY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
/// Pages of the BAR0 register block to map: the controller registers plus the
/// doorbell region above them.
const MMIO_PAGES: u64 = 8;
const PAGE_SIZE: u64 = 4096;

// ---- DMA structures, page-aligned identity-mapped statics ---------------

/// One 4 KiB page. The admin SQ (64 * 64 = 4096 bytes) exactly fills one; the
/// admin CQ (64 * 16) and the IDENTIFY result each need one aligned page.
#[repr(C, align(4096))]
struct Page([u8; 4096]);

static mut ADMIN_SQ: Page = Page([0; 4096]);
static mut ADMIN_CQ: Page = Page([0; 4096]);
static mut IDENTIFY: Page = Page([0; 4096]);

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

/// Is the device at this location an NVMe controller (class 01h/08h, prog-IF 02h)?
fn is_nvme(location: PciLocation) -> bool {
    // SAFETY: configuration reads have no side effects.
    let id = unsafe { pci_read32(location.bus, location.device, location.function, 0x00) };
    if id & 0xffff == 0xffff {
        return false;
    }
    let class = unsafe { pci_read32(location.bus, location.device, location.function, 0x08) };
    (class >> 24) & 0xff == 0x01 && (class >> 16) & 0xff == 0x08 && (class >> 8) & 0xff == 0x02
}

/// A brought-up NVMe controller: its BAR0 base and doorbell stride in bytes.
struct Controller {
    base: u64,
    doorbell_stride: u64,
    depth: u32,
}

/// Enable memory space + bus master and return the controller's 64-bit BAR0 base.
fn map_bar0(location: PciLocation) -> Option<u64> {
    let PciLocation {
        bus,
        device,
        function,
    } = location;
    // SAFETY: enable MMIO + bus mastering, then read BAR0 (a 64-bit memory BAR).
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
    // OVMF places 64-bit BARs above the 4 GiB identity window; map the register
    // pages (uncached, non-executable) at their physical address before touching
    // them. A BAR that already falls inside the identity window is used as is.
    if base >= IDENTITY_LIMIT {
        let flags = PageTableFlags::WRITABLE
            .union(PageTableFlags::CACHE_DISABLE)
            .union(PageTableFlags::NO_EXECUTE);
        for page in 0..MMIO_PAGES {
            let addr = base + page * PAGE_SIZE;
            // SAFETY: identity-map (virt == phys) one device-MMIO page; the frame
            // is the controller's own BAR, owned by this driver while it runs.
            if unsafe { map_page(addr, addr, flags) }.is_err() {
                return None;
            }
        }
    }
    Some(base)
}

/// Reset, configure the admin queue pair, and enable the controller.
fn bring_up(base: u64) -> Option<Controller> {
    // SAFETY: BAR0 is the controller's identity-mapped MMIO window.
    let (cap_low, cap_high) =
        unsafe { (mmio_read32(base, REG_CAP), mmio_read32(base, REG_CAP + 4)) };
    let mqes = cap_low & 0xffff; // maximum queue entries, zero-based
    let doorbell_stride = 4u64 << (cap_high & 0xf); // CAP.DSTRD
    let depth = if mqes + 1 < QUEUE_DEPTH {
        mqes + 1
    } else {
        QUEUE_DEPTH
    };
    if depth < 2 {
        return None;
    }

    let sq_phys = core::ptr::addr_of!(ADMIN_SQ) as u64;
    let cq_phys = core::ptr::addr_of!(ADMIN_CQ) as u64;

    // SAFETY: MMIO on a controller that exists; the queue bases are page-aligned
    // statics the identity map covers.
    unsafe {
        // Disable, then wait for the controller to report not-ready.
        mmio_write32(base, REG_CC, 0);
        let mut budget = 10_000_000u32;
        while mmio_read32(base, REG_CSTS) & CSTS_RDY != 0 {
            budget -= 1;
            if budget == 0 {
                return None;
            }
            core::hint::spin_loop();
        }

        // Admin queue attributes: submission and completion sizes (zero-based).
        mmio_write32(base, REG_AQA, ((depth - 1) << 16) | (depth - 1));
        mmio_write32(base, REG_ASQ, sq_phys as u32);
        mmio_write32(base, REG_ASQ + 4, (sq_phys >> 32) as u32);
        mmio_write32(base, REG_ACQ, cq_phys as u32);
        mmio_write32(base, REG_ACQ + 4, (cq_phys >> 32) as u32);

        // Enable, then wait for ready (or a fatal status).
        mmio_write32(base, REG_CC, CC_ENABLE);
        let mut budget = 10_000_000u32;
        loop {
            let csts = mmio_read32(base, REG_CSTS);
            if csts & CSTS_CFS != 0 {
                return None;
            }
            if csts & CSTS_RDY != 0 {
                break;
            }
            budget -= 1;
            if budget == 0 {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    Some(Controller {
        base,
        doorbell_stride,
        depth,
    })
}

/// One submission/completion queue pair and where the driver is in it. The
/// completion queue starts zeroed, so the first pass through it expects phase 1;
/// every wrap flips the expected phase.
struct Queue {
    sq: *mut u32,
    cq: *const u32,
    id: u64,
    depth: u32,
    tail: u32,
    head: u32,
    phase: u32,
    next_cid: u16,
}

impl Queue {
    const fn new(sq: *mut u32, cq: *const u32, id: u64, depth: u32) -> Self {
        Self {
            sq,
            cq,
            id,
            depth,
            tail: 0,
            head: 0,
            phase: 1,
            next_cid: 1,
        }
    }

    /// Submit one command (CDW0's opcode byte plus dwords 1..16) and wait for
    /// its completion; returns the 15-bit status (0 = success).
    ///
    /// # Safety
    /// CPL0; the controller is enabled and both queue rings plus every buffer the
    /// command points at are identity-mapped and live for the whole call.
    unsafe fn submit(
        &mut self,
        controller: &Controller,
        opcode: u8,
        dwords: [u32; 16],
    ) -> Result<u16, &'static str> {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1).max(1);
        let entry = unsafe { self.sq.add(self.tail as usize * 16) };
        // SAFETY: slot `tail` of the identity-mapped submission ring.
        unsafe {
            for (index, value) in dwords.iter().enumerate() {
                entry.add(index).write_volatile(*value);
            }
            entry.write_volatile(u32::from(opcode) | (u32::from(cid) << 16));
        }
        self.tail = (self.tail + 1) % self.depth;
        // SAFETY: SQ tail doorbell of this queue.
        unsafe {
            core::arch::asm!("mfence", options(nostack, preserves_flags));
            mmio_write32(
                controller.base,
                DOORBELL_BASE + 2 * self.id * controller.doorbell_stride,
                self.tail,
            );
        }
        // CQE dword 3: [15:0] CID, [16] phase, [31:17] status.
        let mut budget = 200_000_000u32;
        let dword3 = loop {
            // SAFETY: completion entry `head` of the identity-mapped CQ.
            let dword3 = unsafe { self.cq.add(self.head as usize * 4 + 3).read_volatile() };
            if (dword3 >> 16) & 1 == self.phase {
                break dword3;
            }
            budget -= 1;
            if budget == 0 {
                return Err("no_completion");
            }
            core::hint::spin_loop();
        };
        self.head += 1;
        if self.head == self.depth {
            self.head = 0;
            self.phase ^= 1;
        }
        // SAFETY: CQ head doorbell of this queue.
        unsafe {
            mmio_write32(
                controller.base,
                DOORBELL_BASE + (2 * self.id + 1) * controller.doorbell_stride,
                self.head,
            );
        }
        if dword3 & 0xffff != u32::from(cid) {
            return Err("wrong_cid");
        }
        Ok(((dword3 >> 17) & 0x7fff) as u16)
    }
}

fn prp_dwords(nsid: u32, prp1: u64) -> [u32; 16] {
    let mut dwords = [0u32; 16];
    dwords[1] = nsid;
    dwords[6] = prp1 as u32;
    dwords[7] = (prp1 >> 32) as u32;
    dwords
}

impl Controller {
    /// Issue IDENTIFY CONTROLLER into the `IDENTIFY` page, returning the 15-bit
    /// status field (0 on success).
    ///
    /// # Safety
    /// CPL0; the controller is enabled and the DMA statics are identity-mapped.
    unsafe fn identify_controller(&self, admin: &mut Queue) -> Result<u16, &'static str> {
        let mut dwords = prp_dwords(0, core::ptr::addr_of!(IDENTIFY) as u64);
        dwords[10] = IDENTIFY_CNS_CONTROLLER;
        // SAFETY: caller's contract.
        unsafe { admin.submit(self, OPCODE_IDENTIFY, dwords) }
    }
}

/// Scan PCI for an NVMe controller and bring up the first one found.
fn init() -> Option<Controller> {
    for bus in 0u16..256 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                let location = PciLocation {
                    bus: bus as u8,
                    device,
                    function,
                };
                if !is_nvme(location) {
                    continue;
                }
                debug_write("AW_NVME_FOUND bus=");
                debug_write_u64(u64::from(location.bus));
                debug_write(" device=");
                debug_write_u64(u64::from(location.device));
                debug_write(" function=");
                debug_write_u64(u64::from(location.function));
                debug_write("\n");
                let base = map_bar0(location)?;
                debug_write("AW_NVME_BAR0 base=");
                debug_write_hex_u64(base);
                debug_write("\n");
                return bring_up(base);
            }
        }
    }
    debug_write("AW_NVME_UNAVAILABLE reason=no_controller\n");
    None
}

/// Print an ASCII field with trailing spaces and NULs trimmed, and report whether
/// it was non-empty and entirely printable ASCII.
fn write_ascii_field(field: &[u8]) -> bool {
    let mut end = field.len();
    while end > 0 && (field[end - 1] == b' ' || field[end - 1] == 0) {
        end -= 1;
    }
    if end == 0 {
        return false;
    }
    let mut printable = true;
    for &byte in &field[..end] {
        if !(0x20..=0x7e).contains(&byte) {
            printable = false;
            break;
        }
        let one = [byte];
        // SAFETY: a single 0x20..=0x7e byte is valid single-byte UTF-8.
        debug_write(unsafe { core::str::from_utf8_unchecked(&one) });
    }
    printable
}

/// Prove an NVMe controller bring-up: enable it, run IDENTIFY CONTROLLER, and
/// read back the model number the controller wrote by DMA - real content, not a
/// status bit. Prints `AW_NVME_UNAVAILABLE` and returns if no controller is
/// present, so it is safe to call on every boot configuration.
pub fn prove() {
    debug_write("AW_NVME_BEGIN\n");
    // init() prints its own AW_NVME_UNAVAILABLE reason when nothing usable is found.
    let Some(controller) = init() else {
        return;
    };
    debug_write("AW_NVME_ENABLED depth=");
    debug_write_u64(u64::from(controller.depth));
    debug_write("\n");

    let mut admin = Queue::new(
        core::ptr::addr_of_mut!(ADMIN_SQ) as *mut u32,
        core::ptr::addr_of!(ADMIN_CQ) as *const u32,
        0,
        controller.depth,
    );
    // SAFETY: CPL0; the controller is enabled and the DMA statics are live.
    let status = match unsafe { controller.identify_controller(&mut admin) } {
        Ok(status) => status,
        Err(reason) => {
            debug_write("AW_NVME_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };
    if status != 0 {
        debug_write("AW_NVME_FAIL reason=status sc=");
        debug_write_hex_u64(u64::from(status));
        debug_write("\n");
        return;
    }

    // Identify Controller structure: model number is 40 bytes at offset 24.
    let identify = core::ptr::addr_of!(IDENTIFY) as *const u8;
    let mut model = [0u8; 40];
    // SAFETY: the controller wrote a full 4 KiB page into the IDENTIFY static.
    for (index, slot) in model.iter_mut().enumerate() {
        *slot = unsafe { identify.add(24 + index).read_volatile() };
    }

    debug_write("AW_NVME_IDENTIFY_OK model=");
    let printable = write_ascii_field(&model);
    debug_write("\n");
    if printable {
        debug_write("AW_NVME_PROOF_OK\n");
    } else {
        debug_write("AW_NVME_FAIL reason=empty_model\n");
        return;
    }

    // SAFETY: CPL0; same controller and DMA statics.
    match unsafe { prove_block_io(&controller, &mut admin) } {
        Ok(()) => {}
        Err(reason) => {
            debug_write("AW_NVME_IO_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
        }
    }
}

// ---- Block I/O: namespace, I/O queue pair, READ and WRITE (roadmap Phase 3) ----

const OPCODE_CREATE_IO_SQ: u8 = 0x01;
const OPCODE_CREATE_IO_CQ: u8 = 0x05;
// Only the scratch-disk proof writes; the normal boot path never does.
#[cfg_attr(not(feature = "nvme-write-smoke-test"), allow(dead_code))]
const OPCODE_WRITE: u8 = 0x01;
const OPCODE_READ: u8 = 0x02;
const IDENTIFY_CNS_NAMESPACE: u32 = 0;
const NAMESPACE_ID: u32 = 1;
const IO_QUEUE_ID: u64 = 1;

static mut IO_SQ: Page = Page([0; 4096]);
static mut IO_CQ: Page = Page([0; 4096]);
static mut DATA: Page = Page([0; 4096]);
#[cfg(feature = "nvme-write-smoke-test")]
static mut DATA_BACK: Page = Page([0; 4096]);

/// Namespace 1's geometry, from IDENTIFY NAMESPACE.
struct Namespace {
    blocks: u64,
    block_size: u32,
}

/// # Safety
/// CPL0; the controller is enabled and the DMA statics are identity-mapped.
unsafe fn identify_namespace(
    controller: &Controller,
    admin: &mut Queue,
) -> Result<Namespace, &'static str> {
    let mut dwords = prp_dwords(NAMESPACE_ID, core::ptr::addr_of!(IDENTIFY) as u64);
    dwords[10] = IDENTIFY_CNS_NAMESPACE;
    // SAFETY: caller's contract.
    if unsafe { admin.submit(controller, OPCODE_IDENTIFY, dwords) }? != 0 {
        return Err("identify_namespace_status");
    }
    let page = core::ptr::addr_of!(IDENTIFY) as *const u8;
    // SAFETY: the controller wrote the 4 KiB identify-namespace page.
    let (blocks, format) = unsafe {
        let blocks = (page as *const u64).read_volatile();
        let flbas = page.add(26).read_volatile() & 0x0f;
        let format = (page.add(128 + 4 * usize::from(flbas)) as *const u32).read_volatile();
        (blocks, format)
    };
    let lbads = (format >> 16) & 0xff;
    if blocks == 0 || !(9..=12).contains(&lbads) {
        return Err("unsupported_namespace"); // one PRP page must hold one block
    }
    Ok(Namespace {
        blocks,
        block_size: 1 << lbads,
    })
}

/// Create the I/O completion queue, then the submission queue bound to it.
///
/// # Safety
/// CPL0; the controller is enabled and the ring statics are identity-mapped.
unsafe fn create_io_queues(
    controller: &Controller,
    admin: &mut Queue,
) -> Result<Queue, &'static str> {
    let depth = controller.depth.min(64);
    let mut cq = prp_dwords(0, core::ptr::addr_of!(IO_CQ) as u64);
    cq[10] = ((depth - 1) << 16) | IO_QUEUE_ID as u32;
    cq[11] = 1; // physically contiguous, interrupts off (polled)
    // SAFETY: caller's contract.
    if unsafe { admin.submit(controller, OPCODE_CREATE_IO_CQ, cq) }? != 0 {
        return Err("create_io_cq_status");
    }
    let mut sq = prp_dwords(0, core::ptr::addr_of!(IO_SQ) as u64);
    sq[10] = ((depth - 1) << 16) | IO_QUEUE_ID as u32;
    sq[11] = ((IO_QUEUE_ID as u32) << 16) | 1; // bound to CQ 1, physically contiguous
    // SAFETY: caller's contract.
    if unsafe { admin.submit(controller, OPCODE_CREATE_IO_SQ, sq) }? != 0 {
        return Err("create_io_sq_status");
    }
    Ok(Queue::new(
        core::ptr::addr_of_mut!(IO_SQ) as *mut u32,
        core::ptr::addr_of!(IO_CQ) as *const u32,
        IO_QUEUE_ID,
        depth,
    ))
}

/// Read or write one block by DMA through `buffer`.
///
/// # Safety
/// CPL0; `buffer` is an identity-mapped 4 KiB page, `lba` is inside the namespace.
unsafe fn block_io(
    controller: &Controller,
    io: &mut Queue,
    opcode: u8,
    lba: u64,
    buffer: u64,
) -> Result<(), &'static str> {
    let mut dwords = prp_dwords(NAMESPACE_ID, buffer);
    dwords[10] = lba as u32;
    dwords[11] = (lba >> 32) as u32;
    dwords[12] = 0; // number of blocks, zero-based: one block
    // SAFETY: caller's contract.
    match unsafe { io.submit(controller, opcode, dwords) }? {
        0 => Ok(()),
        _ => Err("io_status"),
    }
}

/// Read LBA 0 and check it is the FAT16 boot sector the test disk carries;
/// with `nvme-write-smoke-test`, also write a pattern to a scratch block and
/// read it back into a different buffer.
///
/// # Safety
/// CPL0; the controller is enabled and the DMA statics are identity-mapped.
unsafe fn prove_block_io(controller: &Controller, admin: &mut Queue) -> Result<(), &'static str> {
    // SAFETY: caller's contract, forwarded.
    let namespace = unsafe { identify_namespace(controller, admin) }?;
    debug_write("AW_NVME_NAMESPACE blocks=");
    debug_write_u64(namespace.blocks);
    debug_write(" block_size=");
    debug_write_u64(u64::from(namespace.block_size));
    debug_write("\n");
    // SAFETY: as above.
    let mut io = unsafe { create_io_queues(controller, admin) }?;
    debug_write("AW_NVME_IO_QUEUES_OK depth=");
    debug_write_u64(u64::from(io.depth));
    debug_write("\n");

    let data = core::ptr::addr_of_mut!(DATA) as *mut u8;
    // SAFETY: poison the buffer first, so stale bytes cannot pass for a read.
    unsafe { core::ptr::write_bytes(data, 0xcc, 4096) };
    // SAFETY: LBA 0 exists; DATA is identity-mapped.
    unsafe { block_io(controller, &mut io, OPCODE_READ, 0, data as u64) }?;
    // SAFETY: the controller wrote one block into DATA.
    let (signature, fat16) = unsafe {
        let signature = u16::from(data.add(510).read_volatile())
            | (u16::from(data.add(511).read_volatile()) << 8);
        let mut label = [0u8; 5];
        for (i, byte) in label.iter_mut().enumerate() {
            *byte = data.add(54 + i).read_volatile();
        }
        (signature, &label == b"FAT16")
    };
    debug_write("AW_NVME_READ_OK lba=0 signature=");
    debug_write_hex_u64(u64::from(signature));
    debug_write("\n");
    if signature == 0xaa55 && fat16 {
        debug_write("AW_NVME_READ_PROOF_OK fs=FAT16\n");
        crate::firmware_runtime::pass(aw_generation::RuntimeHealthCheck::Storage);
    }

    #[cfg(feature = "nvme-write-smoke-test")]
    {
        // Scratch disk only: LBA 2 is overwritten.
        const SCRATCH_LBA: u64 = 2;
        let back = core::ptr::addr_of_mut!(DATA_BACK) as *mut u8;
        // SAFETY: fill DATA with a position-dependent pattern, poison DATA_BACK.
        unsafe {
            for i in 0..namespace.block_size as usize {
                data.add(i)
                    .write_volatile((i as u8) ^ 0x5a ^ ((i >> 8) as u8));
            }
            core::ptr::write_bytes(back, 0xcc, 4096);
        }
        if namespace.blocks <= SCRATCH_LBA {
            return Err("scratch_too_small");
        }
        // SAFETY: scratch LBA inside the namespace; both buffers identity-mapped.
        unsafe {
            block_io(controller, &mut io, OPCODE_WRITE, SCRATCH_LBA, data as u64)?;
            block_io(controller, &mut io, OPCODE_READ, SCRATCH_LBA, back as u64)?;
        }
        // SAFETY: both buffers hold at least one block.
        let same = (0..namespace.block_size as usize)
            .all(|i| unsafe { data.add(i).read_volatile() == back.add(i).read_volatile() });
        if same {
            debug_write("AW_NVME_WRITE_PROOF_OK lba=2\n");
        } else {
            return Err("readback_mismatch");
        }
    }
    Ok(())
}
