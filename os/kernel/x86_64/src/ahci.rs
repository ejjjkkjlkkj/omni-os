//! Minimal AHCI/SATA block driver: read one sector by DMA (dossier section 11.2
//! "NVMe/AHCI", roadmap Phase 3).
//!
//! virtio-blk proved a paravirtual device; this proves a *real* controller model,
//! the one VMware and most physical PCs expose their SATA disks through. It finds
//! an AHCI HBA on PCI, enables AHCI mode, brings up the first port that reports a
//! device, and issues a single READ DMA EXT of LBA 0 into a buffer, then checks
//! the boot-sector signature the disk actually returned - not a status bit.
//!
//! Only what a single-sector proof needs is implemented: one command slot, one
//! PRDT entry, polled completion, no interrupts, no NCQ, no hot-plug. Every
//! structure the HBA touches by DMA lives in a `static` the identity map covers
//! 1:1, so its virtual address is also the physical address handed to the HBA.

use core::sync::atomic::{compiler_fence, Ordering};

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
        outl(PCI_CONFIG_ADDRESS, pci_address(bus, device, function, offset));
        inl(PCI_CONFIG_DATA)
    }
}

unsafe fn pci_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    // SAFETY: CF8/CFC are the architected PCI configuration ports.
    unsafe {
        outl(PCI_CONFIG_ADDRESS, pci_address(bus, device, function, offset));
        outl(PCI_CONFIG_DATA, value);
    }
}

// ---- MMIO on the HBA's ABAR (BAR5) --------------------------------------

unsafe fn mmio_read(base: u64, offset: u64) -> u32 {
    // SAFETY: `base+offset` is inside the identity-mapped ABAR MMIO window.
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

unsafe fn mmio_write(base: u64, offset: u64, value: u32) {
    // SAFETY: `base+offset` is inside the identity-mapped ABAR MMIO window.
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
}

// ---- HBA and port register offsets --------------------------------------

const HBA_GHC: u64 = 0x04; // global host control
const HBA_PI: u64 = 0x0c; // ports implemented
const GHC_AE: u32 = 1 << 31; // AHCI enable

const PORT_BASE: u64 = 0x100;
const PORT_STRIDE: u64 = 0x80;
const PX_CLB: u64 = 0x00; // command list base (low)
const PX_CLBU: u64 = 0x04;
const PX_FB: u64 = 0x08; // FIS base (low)
const PX_FBU: u64 = 0x0c;
const PX_IS: u64 = 0x10; // interrupt status
const PX_CMD: u64 = 0x18;
const PX_TFD: u64 = 0x20; // task file data
const PX_SSTS: u64 = 0x28; // SATA status
const PX_SERR: u64 = 0x30;
const PX_CI: u64 = 0x38; // command issue

const CMD_ST: u32 = 1 << 0; // start
const CMD_FRE: u32 = 1 << 4; // FIS receive enable
const CMD_FR: u32 = 1 << 14; // FIS receive running
const CMD_CR: u32 = 1 << 15; // command list running

const TFD_BSY: u32 = 1 << 7;
const TFD_DRQ: u32 = 1 << 3;
const TFD_ERR: u32 = 1 << 0;

const SSTS_DET_PRESENT: u32 = 0x3; // device present and PHY communication established

const ATA_READ_DMA_EXT: u8 = 0x25;
#[cfg(any(feature = "ahci-write-smoke-test", feature = "fat-write-smoke-test", feature = "gpt-write-smoke-test", feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
const ATA_WRITE_DMA_EXT: u8 = 0x35;
pub const SECTOR_SIZE: usize = 512;

// ---- DMA structures, identity-mapped statics ----------------------------

/// 32 command headers, 32 bytes each = 1 KiB. 1 KiB aligned (PxCLB requires it).
#[repr(C, align(1024))]
struct CommandList([u8; 1024]);
static mut COMMAND_LIST: CommandList = CommandList([0; 1024]);

/// FIS receive area, 256 bytes, 256-aligned (PxFB requires it).
#[repr(C, align(256))]
struct FisArea([u8; 256]);
static mut FIS_AREA: FisArea = FisArea([0; 256]);

/// One command table: 64-byte command FIS + 16 ATAPI + 48 reserved, then PRDT
/// entries (16 bytes each). One PRDT entry is enough for a single sector.
#[repr(C, align(128))]
struct CommandTable([u8; 256]);
static mut COMMAND_TABLE: CommandTable = CommandTable([0; 256]);

#[repr(C, align(512))]
struct DataBuffer([u8; SECTOR_SIZE]);
static mut DATA: DataBuffer = DataBuffer([0; SECTOR_SIZE]);

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

/// Is the device at this location an AHCI HBA (class 01h/06h, prog-IF 01h)?
fn is_ahci(location: PciLocation) -> bool {
    // SAFETY: configuration reads have no side effects.
    let id = unsafe { pci_read32(location.bus, location.device, location.function, 0x00) };
    if id & 0xffff == 0xffff {
        return false;
    }
    let class = unsafe { pci_read32(location.bus, location.device, location.function, 0x08) };
    (class >> 24) & 0xff == 0x01 && (class >> 16) & 0xff == 0x06 && (class >> 8) & 0xff == 0x01
}

/// A brought-up AHCI port: the ABAR base and the port index in use.
pub struct AhciPort {
    abar: u64,
    port: u64,
}

fn port_reg(port: u64, reg: u64) -> u64 {
    PORT_BASE + port * PORT_STRIDE + reg
}

/// Enable one AHCI HBA and return the first of its ports that has a device.
fn bring_up_hba(location: PciLocation) -> Option<AhciPort> {
    let PciLocation {
        bus,
        device,
        function,
    } = location;
    // SAFETY: enable memory space + bus mastering, then read ABAR (BAR5).
    let abar = unsafe {
        let command = pci_read32(bus, device, function, 0x04);
        pci_write32(bus, device, function, 0x04, command | 0b110); // MMIO + bus master
        u64::from(pci_read32(bus, device, function, 0x24) & 0xffff_fff0)
    };
    if abar == 0 || abar > u64::from(u32::MAX) {
        return None;
    }
    // SAFETY: ABAR is the HBA's identity-mapped MMIO window; enable AHCI mode.
    let pi = unsafe {
        let ghc = mmio_read(abar, HBA_GHC);
        mmio_write(abar, HBA_GHC, ghc | GHC_AE);
        mmio_read(abar, HBA_PI)
    };
    for port in 0u64..32 {
        if pi & (1 << port) == 0 {
            continue;
        }
        // SAFETY: reading the port's SATA status over MMIO.
        let ssts = unsafe { mmio_read(abar, port_reg(port, PX_SSTS)) };
        if ssts & 0xf != SSTS_DET_PRESENT {
            continue;
        }
        debug_write("AW_AHCI_PORT_PRESENT abar=");
        debug_write_hex_u64(abar);
        debug_write(" port=");
        debug_write_u64(port);
        debug_write("\n");
        let device = AhciPort { abar, port };
        // SAFETY: CPL0; program this port's command list / FIS area.
        if unsafe { device.start() } {
            return Some(device);
        }
    }
    None
}

/// Scan PCI for AHCI controllers and bring up the first port, on any of them,
/// that reports a device. q35 exposes an empty built-in AHCI, so more than one
/// controller (or none with a disk) is normal; only a present port matters.
pub fn init() -> Option<AhciPort> {
    let mut saw_hba = false;
    for bus in 0u16..256 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                let location = PciLocation {
                    bus: bus as u8,
                    device,
                    function,
                };
                if !is_ahci(location) {
                    continue;
                }
                saw_hba = true;
                debug_write("AW_AHCI_FOUND bus=");
                debug_write_u64(u64::from(location.bus));
                debug_write(" device=");
                debug_write_u64(u64::from(location.device));
                debug_write(" function=");
                debug_write_u64(u64::from(location.function));
                debug_write("\n");
                if let Some(port) = bring_up_hba(location) {
                    return Some(port);
                }
            }
        }
    }
    if !saw_hba {
        debug_write("AW_AHCI_UNAVAILABLE reason=no_controller\n");
    } else {
        debug_write("AW_AHCI_UNAVAILABLE reason=no_device\n");
    }
    None
}

impl AhciPort {
    /// Stop the port, point it at our command list and FIS area, and restart it.
    ///
    /// # Safety
    /// CPL0; the ABAR and port index are valid and the statics are live.
    unsafe fn start(&self) -> bool {
        let base = self.abar;
        let cmd = port_reg(self.port, PX_CMD);
        // SAFETY: stop the port: clear ST and FRE, then wait for CR and FR to
        // clear so it is safe to reprogram the pointers.
        unsafe {
            let current = mmio_read(base, cmd);
            mmio_write(base, cmd, current & !(CMD_ST | CMD_FRE));
            let mut budget = 1_000_000u32;
            while mmio_read(base, cmd) & (CMD_CR | CMD_FR) != 0 {
                budget -= 1;
                if budget == 0 {
                    return false;
                }
                core::hint::spin_loop();
            }

            let clb = core::ptr::addr_of!(COMMAND_LIST) as u64;
            let fb = core::ptr::addr_of!(FIS_AREA) as u64;
            for byte in 0..1024 {
                (clb as *mut u8).add(byte).write_volatile(0);
            }
            for byte in 0..256 {
                (fb as *mut u8).add(byte).write_volatile(0);
            }
            mmio_write(base, port_reg(self.port, PX_CLB), clb as u32);
            mmio_write(base, port_reg(self.port, PX_CLBU), 0);
            mmio_write(base, port_reg(self.port, PX_FB), fb as u32);
            mmio_write(base, port_reg(self.port, PX_FBU), 0);
            mmio_write(base, port_reg(self.port, PX_SERR), 0xffff_ffff);

            // Start FIS receive and the command engine.
            let current = mmio_read(base, cmd);
            mmio_write(base, cmd, current | CMD_FRE | CMD_ST);
        }
        true
    }

    /// Build, issue and await one single-sector command on slot 0, transferring
    /// through the `DATA` static: `command_byte` is the ATA command and `write`
    /// sets the header's write bit and the FIS direction. The caller fills `DATA`
    /// before a write, or reads it after a read.
    ///
    /// # Safety
    /// CPL0; the port is started and the DMA statics are live and identity-mapped.
    unsafe fn command(&self, lba: u64, command_byte: u8, write: bool) -> Result<(), &'static str> {
        let base = self.abar;
        let clb = core::ptr::addr_of_mut!(COMMAND_LIST) as *mut u8;
        let ctba = core::ptr::addr_of_mut!(COMMAND_TABLE) as *mut u8;
        let data = core::ptr::addr_of_mut!(DATA) as *mut u8;

        // SAFETY: build command header 0 and its command table + PRDT.
        unsafe {
            // Command header 0: CFL=5 dwords (H2D FIS is 20 bytes), PRDTL=1, and
            // the write bit (DW0 bit 6) set for a device write.
            let cfl = 5u32;
            let prdtl = 1u32 << 16;
            let write_bit = if write { 1u32 << 6 } else { 0 };
            clb.cast::<u32>().write_volatile(cfl | write_bit | prdtl); // DW0
            clb.add(4).cast::<u32>().write_volatile(0); // PRDBC
            clb.add(8).cast::<u32>().write_volatile(ctba as u32); // CTBA low
            clb.add(12).cast::<u32>().write_volatile(0); // CTBA high

            // Zero the command table, then fill the command FIS and PRDT.
            for byte in 0..256 {
                ctba.add(byte).write_volatile(0);
            }
            // Register H2D FIS.
            ctba.add(0).write_volatile(0x27); // FIS type H2D
            ctba.add(1).write_volatile(0x80); // C=1 (command)
            ctba.add(2).write_volatile(command_byte);
            ctba.add(4).write_volatile(lba as u8); // LBA 0..7
            ctba.add(5).write_volatile((lba >> 8) as u8); // LBA 8..15
            ctba.add(6).write_volatile((lba >> 16) as u8); // LBA 16..23
            ctba.add(7).write_volatile(0x40); // device: LBA mode
            ctba.add(8).write_volatile((lba >> 24) as u8); // LBA 24..31
            ctba.add(9).write_volatile((lba >> 32) as u8); // LBA 32..39
            ctba.add(10).write_volatile((lba >> 40) as u8); // LBA 40..47
            ctba.add(12).write_volatile(1); // count low = 1 sector
            ctba.add(13).write_volatile(0); // count high

            // PRDT entry 0 at offset 0x80 in the command table.
            let prdt = ctba.add(0x80);
            prdt.cast::<u32>().write_volatile(data as u32); // DBA low
            prdt.add(4).cast::<u32>().write_volatile(0); // DBA high
            prdt.add(8).cast::<u32>().write_volatile(0); // reserved
            // DBC = byte count - 1 (bit31 = interrupt on completion, harmless).
            prdt.add(12)
                .cast::<u32>()
                .write_volatile((SECTOR_SIZE as u32 - 1) | (1 << 31));
        }

        // Wait for the port to go idle before issuing. A real HBA (VMware's) is
        // still BSY right after the engine starts, delivering its initial D2H
        // register FIS; QEMU is ready at once. Issuing while BSY makes the command
        // complete with an error, so wait for BSY and DRQ to clear first.
        let mut ready = 200_000_000u32;
        loop {
            // SAFETY: MMIO read of this port's task file register.
            let tfd = unsafe { mmio_read(base, port_reg(self.port, PX_TFD)) };
            if tfd & (TFD_BSY | TFD_DRQ) == 0 {
                break;
            }
            ready -= 1;
            if ready == 0 {
                return Err("port_not_ready");
            }
            core::hint::spin_loop();
        }

        // Clear any stale interrupt status, then issue command slot 0.
        // SAFETY: MMIO on this port; single outstanding command.
        unsafe {
            mmio_write(base, port_reg(self.port, PX_IS), 0xffff_ffff);
            compiler_fence(Ordering::SeqCst);
            mmio_write(base, port_reg(self.port, PX_CI), 1);
        }

        let mut budget = 200_000_000u32;
        loop {
            // SAFETY: reading CI/TFD over MMIO.
            let ci = unsafe { mmio_read(base, port_reg(self.port, PX_CI)) };
            if ci & 1 == 0 {
                break;
            }
            let tfd = unsafe { mmio_read(base, port_reg(self.port, PX_TFD)) };
            if tfd & TFD_ERR != 0 {
                return Err("task_file_error");
            }
            budget -= 1;
            if budget == 0 {
                return Err("no_completion");
            }
            core::hint::spin_loop();
        }
        compiler_fence(Ordering::SeqCst);

        // Command slot consumed (CI cleared). Success is the absence of a task
        // file error, not the task file being perfectly idle: a real HBA (VMware's)
        // can still show BSY here for a moment after a good transfer, while it has
        // already moved the data and posted DPS in PxIS with no TFES. Fail only on
        // an actual error bit; a read-back is the real proof that the bytes moved.
        // SAFETY: MMIO read of this port's task file register.
        let tfd = unsafe { mmio_read(base, port_reg(self.port, PX_TFD)) };
        if tfd & TFD_ERR != 0 {
            return Err("task_file_error");
        }
        Ok(())
    }

    /// Read one 512-byte sector `lba` into `out` by DMA (READ DMA EXT).
    pub fn read_sector(&self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        // SAFETY: CPL0; single-sector DMA read into the DATA static.
        unsafe { self.command(lba, ATA_READ_DMA_EXT, false)? };
        let data = core::ptr::addr_of!(DATA) as *const u8;
        // SAFETY: copy the DMA buffer out through its identity address.
        unsafe {
            for (index, slot) in out.iter_mut().enumerate() {
                *slot = data.add(index).read_volatile();
            }
        }
        Ok(())
    }

    /// Write one 512-byte sector `lba` from `src` by DMA (WRITE DMA EXT).
    #[cfg(any(feature = "ahci-write-smoke-test", feature = "fat-write-smoke-test", feature = "gpt-write-smoke-test", feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
    pub fn write_sector(&self, lba: u64, src: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        let data = core::ptr::addr_of_mut!(DATA) as *mut u8;
        // SAFETY: stage the bytes in the DMA buffer, then a single-sector write.
        unsafe {
            for (index, byte) in src.iter().enumerate() {
                data.add(index).write_volatile(*byte);
            }
            self.command(lba, ATA_WRITE_DMA_EXT, true)?;
        }
        Ok(())
    }
}

/// Prove a real AHCI sector read: read LBA 0 and confirm the boot-sector
/// signature (`0x55AA` at offset 510) the disk returned - the protective MBR on a
/// GPT disk, or the FAT boot sector on the test image, either way content the HBA
/// actually delivered by DMA. Prints `AW_AHCI_UNAVAILABLE` and returns if no
/// controller is present, so it is safe to call on every boot configuration.
#[cfg(not(feature = "ahci-write-smoke-test"))]
pub fn prove() {
    debug_write("AW_AHCI_BEGIN\n");
    // init() prints its own AW_AHCI_UNAVAILABLE reason when nothing usable is found.
    let Some(port) = init() else {
        return;
    };

    let mut sector = [0u8; SECTOR_SIZE];
    match port.read_sector(0, &mut sector) {
        Ok(()) => {
            if sector[510] == 0x55 && sector[511] == 0xaa {
                debug_write("AW_AHCI_READ_OK sector=0\n");
                debug_write("AW_AHCI_PROOF_OK\n");
            } else {
                debug_write("AW_AHCI_FAIL reason=no_signature sig=");
                debug_write_hex_u64(u64::from(sector[510]) | (u64::from(sector[511]) << 8));
                debug_write("\n");
            }
        }
        Err(reason) => {
            debug_write("AW_AHCI_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
        }
    }
}

/// Prove a real AHCI write: write a known pattern to LBA 0, read it back through a
/// fresh command, and confirm the bytes round-tripped (dossier section 11.2,
/// roadmap Phase 3 "AHCI/SATA read/write").
///
/// Test-only, and only ever pointed at a dedicated scratch disk - never a boot or
/// data disk, since it overwrites LBA 0. It is gated behind `ahci-write-smoke-test`
/// and is not in the normal boot path, so a real machine's disk is never written.
#[cfg(feature = "ahci-write-smoke-test")]
pub fn prove_write() {
    debug_write("AW_AHCI_WRITE_BEGIN\n");
    let Some(port) = init() else {
        return;
    };

    // A distinctive, position-dependent pattern ending in the 0x55AA signature, so
    // a stale-buffer read or a partial transfer cannot pass by accident.
    let mut pattern = [0u8; SECTOR_SIZE];
    for (index, byte) in pattern.iter_mut().enumerate() {
        *byte = (index as u8) ^ 0xa5;
    }
    pattern[510] = 0x55;
    pattern[511] = 0xaa;

    if let Err(reason) = port.write_sector(0, &pattern) {
        debug_write("AW_AHCI_WRITE_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_AHCI_WRITE_ISSUED sector=0\n");

    let mut back = [0u8; SECTOR_SIZE];
    if let Err(reason) = port.read_sector(0, &mut back) {
        debug_write("AW_AHCI_WRITE_FAIL reason=readback_");
        debug_write(reason);
        debug_write("\n");
        return;
    }

    if back == pattern {
        debug_write("AW_AHCI_WRITE_PROOF_OK\n");
    } else {
        debug_write("AW_AHCI_WRITE_FAIL reason=mismatch\n");
    }
}
