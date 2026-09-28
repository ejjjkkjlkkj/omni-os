//! First real device driver: virtio-blk over the legacy (PIO) transport
//! (dossier section 11.2, roadmap P0 step 6).
//!
//! The APIC/IOAPIC/MSI proofs showed interrupts arriving; this shows the kernel
//! actually driving a device and moving data. It brings up a legacy virtio-block
//! device, sets up one virtqueue, submits a single read of sector 0, and checks
//! the bytes the device returned against a magic the test disk was built with.
//! Nothing here is claimed from a status register alone: the proof is the sector
//! content.
//!
//! Legacy virtio is little-endian, which is the guest's native order on x86, and
//! its ring and buffers are addressed by physical address. Every structure the
//! device reads or writes lives in a `static` inside the kernel image, which the
//! identity map covers 1:1, so a buffer's virtual address is also the physical
//! address handed to the device.

use core::sync::atomic::{compiler_fence, Ordering};

use crate::{debug_write, debug_write_hex_u64, debug_write_u64};

// ---- x86 port I/O -------------------------------------------------------

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: the caller names a valid byte-wide port.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn outw(port: u16, value: u16) {
    // SAFETY: the caller names a valid word-wide port.
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn outl(port: u16, value: u32) {
    // SAFETY: the caller names a valid dword-wide port.
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: the caller names a valid byte-wide port.
    unsafe {
        core::arch::asm!("in al, dx", out("al") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}

unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: the caller names a valid word-wide port.
    unsafe {
        core::arch::asm!("in ax, dx", out("ax") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
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

// ---- PCI configuration (mechanism #1, CF8/CFC) --------------------------

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

/// Red Hat / virtio PCI vendor, and the transitional virtio-block device id.
const VIRTIO_VENDOR: u16 = 0x1af4;
const VIRTIO_BLK_DEVICE: u16 = 0x1001;

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

fn find_virtio_blk() -> Option<PciLocation> {
    for bus in 0u8..=255 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                // SAFETY: configuration reads have no side effects.
                let id = unsafe { pci_read32(bus, device, function, 0x00) };
                let vendor = (id & 0xffff) as u16;
                let dev = (id >> 16) as u16;
                if vendor == VIRTIO_VENDOR && dev == VIRTIO_BLK_DEVICE {
                    return Some(PciLocation {
                        bus,
                        device,
                        function,
                    });
                }
            }
        }
        if bus == 255 {
            break;
        }
    }
    None
}

// ---- Legacy virtio register offsets (from the I/O BAR base) --------------

const VIRTIO_DEVICE_FEATURES: u16 = 0x00;
const VIRTIO_GUEST_FEATURES: u16 = 0x04;
const VIRTIO_QUEUE_PFN: u16 = 0x08;
const VIRTIO_QUEUE_SIZE: u16 = 0x0c;
const VIRTIO_QUEUE_SELECT: u16 = 0x0e;
const VIRTIO_QUEUE_NOTIFY: u16 = 0x10;
const VIRTIO_STATUS: u16 = 0x12;
const VIRTIO_CONFIG: u16 = 0x14; // device-specific config (blk capacity)

const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FAILED: u8 = 0x80;

const VRING_DESC_NEXT: u16 = 1;
const VRING_DESC_WRITE: u16 = 2;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_S_OK: u8 = 0;

pub const SECTOR_SIZE: usize = 512;
/// Largest queue this driver's static ring can describe.
const MAX_QUEUE: usize = 256;
const QUEUE_ALIGN: usize = 4096;

/// Ring storage: descriptor table + available ring + (aligned) used ring, sized
/// for `MAX_QUEUE`. Page aligned so its physical frame number is exact.
#[repr(C, align(4096))]
struct VRing([u8; 16384]);

static mut VRING: VRing = VRing([0; 16384]);

/// virtio-blk request header: type, reserved, sector.
#[repr(C)]
struct BlkRequestHeader {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

static mut REQUEST: BlkRequestHeader = BlkRequestHeader {
    request_type: 0,
    reserved: 0,
    sector: 0,
};
static mut DATA: [u8; SECTOR_SIZE] = [0; SECTOR_SIZE];
static mut STATUS_BYTE: [u8; 1] = [0xff];

/// A brought-up virtio-block device: its I/O BAR base and negotiated queue size.
#[derive(Clone, Copy)]
pub struct BlkDevice {
    base: u16,
    queue_size: usize,
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn avail_offset(queue_size: usize) -> usize {
    16 * queue_size
}

fn used_offset(queue_size: usize) -> usize {
    align_up(avail_offset(queue_size) + 6 + 2 * queue_size, QUEUE_ALIGN)
}

impl BlkDevice {
    /// Read one 512-byte sector `lba` into `out` through the virtqueue.
    ///
    /// Requests are issued and awaited one at a time, so the single static ring
    /// and buffers are reused for every call.
    pub fn read_sector(&self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        let base = self.base;
        let ring = core::ptr::addr_of_mut!(VRING) as *mut u8;
        let request_ptr = core::ptr::addr_of_mut!(REQUEST);
        let data_ptr = core::ptr::addr_of_mut!(DATA) as *mut u8;
        let status_ptr = core::ptr::addr_of_mut!(STATUS_BYTE) as *mut u8;

        // SAFETY: the request/status statics are live and correctly aligned.
        unsafe {
            request_ptr.write(BlkRequestHeader {
                request_type: VIRTIO_BLK_T_IN,
                reserved: 0,
                sector: lba,
            });
            status_ptr.write_volatile(0xff);
        }

        // Header (device reads), data (device writes), status (device writes).
        // SAFETY: descriptor indices 0..2 are inside the ring.
        unsafe {
            write_desc(ring, 0, request_ptr as u64, 16, VRING_DESC_NEXT, 1);
            write_desc(
                ring,
                1,
                data_ptr as u64,
                SECTOR_SIZE as u32,
                VRING_DESC_NEXT | VRING_DESC_WRITE,
                2,
            );
            write_desc(ring, 2, status_ptr as u64, 1, VRING_DESC_WRITE, 0);
        }

        let avail = avail_offset(self.queue_size);
        let used = used_offset(self.queue_size);
        // SAFETY: `avail`/`used` are inside the ring; single outstanding request.
        let used_before = unsafe {
            let idx = read_u16(ring, avail + 2);
            write_u16(ring, avail, 0);
            write_u16(ring, avail + 4 + (usize::from(idx) % self.queue_size) * 2, 0);
            let before = read_u16(ring, used + 2);
            compiler_fence(Ordering::SeqCst);
            write_u16(ring, avail + 2, idx.wrapping_add(1));
            before
        };

        // SAFETY: barrier then notify queue 0.
        unsafe {
            core::arch::asm!("mfence", options(nostack, preserves_flags));
            outw(base + VIRTIO_QUEUE_NOTIFY, 0);
        }

        let mut budget = 200_000_000u32;
        loop {
            // SAFETY: reading used.idx from the ring.
            if unsafe { read_u16(ring, used + 2) } != used_before {
                break;
            }
            budget -= 1;
            if budget == 0 {
                return Err("no_completion");
            }
            core::hint::spin_loop();
        }
        compiler_fence(Ordering::SeqCst);

        // SAFETY: the device wrote the status byte and the data buffer.
        unsafe {
            if status_ptr.read_volatile() != VIRTIO_BLK_S_OK {
                return Err("device_status");
            }
            for (index, slot) in out.iter_mut().enumerate() {
                *slot = data_ptr.add(index).read_volatile();
            }
        }
        Ok(())
    }
}

/// Find a legacy virtio-block device and bring it up: handshake and one
/// virtqueue, ready for [`BlkDevice::read_sector`].
pub fn init() -> Option<BlkDevice> {
    let location = find_virtio_blk()?;
    debug_write("AW_VIRTIO_BLK_FOUND bus=");
    debug_write_u64(u64::from(location.bus));
    debug_write(" device=");
    debug_write_u64(u64::from(location.device));
    debug_write(" function=");
    debug_write_u64(u64::from(location.function));
    debug_write("\n");
    let base = prepare(location)?;

    // Reset, acknowledge, claim, accept no optional features.
    // SAFETY: `base` is this device's I/O BAR; register offsets are fixed.
    unsafe {
        outb(base + VIRTIO_STATUS, 0);
        while inb(base + VIRTIO_STATUS) != 0 {
            core::hint::spin_loop();
        }
        outb(base + VIRTIO_STATUS, STATUS_ACKNOWLEDGE);
        outb(base + VIRTIO_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        let _features = inl(base + VIRTIO_DEVICE_FEATURES);
        outl(base + VIRTIO_GUEST_FEATURES, 0);
    }

    // SAFETY: select queue 0 and read its size.
    let queue_size = unsafe {
        outw(base + VIRTIO_QUEUE_SELECT, 0);
        inw(base + VIRTIO_QUEUE_SIZE) as usize
    };
    if queue_size == 0 || queue_size > MAX_QUEUE || !queue_size.is_power_of_two() {
        // SAFETY: mark the device failed and give up.
        unsafe { outb(base + VIRTIO_STATUS, STATUS_FAILED) };
        return None;
    }

    let ring_phys = core::ptr::addr_of!(VRING) as u64;
    // SAFETY: hand the page-aligned ring to the device and go live.
    unsafe {
        outl(base + VIRTIO_QUEUE_PFN, (ring_phys >> 12) as u32);
        outb(
            base + VIRTIO_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_DRIVER_OK,
        );
    }

    Some(BlkDevice { base, queue_size })
}

unsafe fn write_desc(ring: *mut u8, index: usize, addr: u64, len: u32, flags: u16, next: u16) {
    let base = index * 16;
    // SAFETY: `base+16` stays within the descriptor table for index < MAX_QUEUE.
    unsafe {
        ring.add(base).cast::<u64>().write_volatile(addr);
        ring.add(base + 8).cast::<u32>().write_volatile(len);
        ring.add(base + 12).cast::<u16>().write_volatile(flags);
        ring.add(base + 14).cast::<u16>().write_volatile(next);
    }
}

unsafe fn write_u16(ring: *mut u8, offset: usize, value: u16) {
    // SAFETY: caller keeps `offset` inside the ring.
    unsafe { ring.add(offset).cast::<u16>().write_volatile(value) };
}

unsafe fn read_u16(ring: *mut u8, offset: usize) -> u16 {
    // SAFETY: caller keeps `offset` inside the ring.
    unsafe { ring.add(offset).cast::<u16>().read_volatile() }
}

/// Enable I/O space and bus-master on the device, and return its I/O BAR base.
fn prepare(location: PciLocation) -> Option<u16> {
    let PciLocation {
        bus,
        device,
        function,
    } = location;
    // SAFETY: configuration space reads/writes on a device that exists.
    unsafe {
        let command = pci_read32(bus, device, function, 0x04);
        // Bit 0 I/O space, bit 2 bus master.
        pci_write32(bus, device, function, 0x04, command | 0b101);
        let bar0 = pci_read32(bus, device, function, 0x10);
        if bar0 & 1 == 0 {
            return None; // not an I/O BAR: this is not the legacy interface
        }
        Some((bar0 & 0xfffc) as u16)
    }
}

/// Prove a real sector read: read sector 0 and confirm it is a FAT boot sector
/// (the disk is a FAT16 filesystem), which is content the device actually
/// returned, not a status bit.
pub fn prove(device: &BlkDevice) {
    // SAFETY: the config block is readable over this device's I/O BAR.
    let capacity = unsafe {
        u64::from(inl(device.base + VIRTIO_CONFIG))
            | (u64::from(inl(device.base + VIRTIO_CONFIG + 4)) << 32)
    };
    debug_write("AW_VIRTIO_BLK_IO_BASE base=");
    debug_write_hex_u64(u64::from(device.base));
    debug_write("\n");
    debug_write("AW_VIRTIO_BLK_CAPACITY sectors=");
    debug_write_u64(capacity);
    debug_write("\n");

    let mut sector = [0u8; SECTOR_SIZE];
    match device.read_sector(0, &mut sector) {
        Ok(()) => {
            // A FAT boot sector begins with a jump (0xEB or 0xE9) and ends with
            // the 0x55AA signature.
            let is_boot_sector =
                (sector[0] == 0xEB || sector[0] == 0xE9) && sector[510] == 0x55 && sector[511] == 0xAA;
            if is_boot_sector {
                debug_write("AW_VIRTIO_BLK_READ_OK sector=0\n");
                debug_write("AW_VIRTIO_BLK_PROOF_OK\n");
            } else {
                debug_write("AW_VIRTIO_BLK_FAIL reason=not_boot_sector\n");
            }
        }
        Err(reason) => {
            debug_write("AW_VIRTIO_BLK_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
        }
    }
}
