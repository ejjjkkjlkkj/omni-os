//! Second real device driver: virtio-net over the legacy (PIO) transport
//! (dossier section 11.2 "réseau", roadmap P0 step 6).
//!
//! virtio-blk showed the kernel driving one device and one virtqueue. A network
//! card is the first device that both consumes and produces buffers on its own
//! schedule, so it needs two queues - a receiveq the device writes into and a
//! transmitq the device reads from - and a proof that a frame actually left the
//! guest and a *different* frame came back because of it.
//!
//! The proof is an ARP exchange against QEMU's user-mode (SLIRP) network: the
//! guest broadcasts "who has 10.0.2.2" and the SLIRP gateway answers. Three
//! things are asserted, none of them a status bit:
//!
//! * the device's own MAC, read from its config space;
//! * a *negative* window - with receive buffers already posted but nothing sent,
//!   the receive ring stays empty, so any later arrival is a consequence of our
//!   transmit and not ambient traffic (the same shape as the MSI mask proof);
//! * the ARP *reply*: opcode 2, sender protocol address 10.0.2.2, carrying the
//!   gateway's hardware address - a frame the device could only have delivered by
//!   really transmitting ours and receiving the answer.
//!
//! ARP stays at layer 2. The proof then goes one layer up: a routed IPv4
//! datagram - with its own header checksum - carrying an ICMP echo request to
//! the gateway (10.0.2.2), addressed on the wire to the hardware address ARP
//! just resolved. Its reply is attributable more strongly still: the echo reply
//! must carry back our exact identifier, sequence and 16-byte payload, so a
//! matching reply can only be the gateway echoing the request we sent.
//!
//! Legacy virtio is little-endian (the guest's native order on x86) and its ring
//! and buffers are addressed physically. Every structure the device touches lives
//! in a `static` the identity map covers 1:1, so a virtual address is also the
//! physical address handed to the device.
//!
//! The PIO and PCI-config helpers are duplicated from `virtio_blk` on purpose:
//! this keeps the driver a self-contained, independently reviewable commit. They
//! belong in a shared `virtio_pci` module once a third user exists.

use core::sync::atomic::{compiler_fence, Ordering};

use crate::{debug_write, debug_write_u64};

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

/// Red Hat / virtio PCI vendor, and the transitional virtio-net device id.
const VIRTIO_VENDOR: u16 = 0x1af4;
const VIRTIO_NET_DEVICE: u16 = 0x1000;

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

fn find_virtio_net() -> Option<PciLocation> {
    for bus in 0u8..=255 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                // SAFETY: configuration reads have no side effects.
                let id = unsafe { pci_read32(bus, device, function, 0x00) };
                let vendor = (id & 0xffff) as u16;
                let dev = (id >> 16) as u16;
                if vendor == VIRTIO_VENDOR && dev == VIRTIO_NET_DEVICE {
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
// Identical common-config layout to virtio-blk; only the device-specific config
// block at 0x14 differs (here it begins with the six-byte MAC).

const VIRTIO_DEVICE_FEATURES: u16 = 0x00;
const VIRTIO_GUEST_FEATURES: u16 = 0x04;
const VIRTIO_QUEUE_PFN: u16 = 0x08;
const VIRTIO_QUEUE_SIZE: u16 = 0x0c;
const VIRTIO_QUEUE_SELECT: u16 = 0x0e;
const VIRTIO_QUEUE_NOTIFY: u16 = 0x10;
const VIRTIO_STATUS: u16 = 0x12;
const VIRTIO_CONFIG: u16 = 0x14; // device-specific config; MAC[0] lives here

const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FAILED: u8 = 0x80;

const VRING_DESC_WRITE: u16 = 2;

/// The only feature this driver negotiates: a device-supplied MAC address. Every
/// other bit is refused so the ring and header layout stay the legacy defaults -
/// in particular VIRTIO_NET_F_MRG_RXBUF is *not* taken, which keeps the receive
/// header a fixed 10 bytes with no trailing `num_buffers`.
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

/// Queue indices. virtio-net's first two queues are receive then transmit.
const RX_QUEUE: u16 = 0;
const TX_QUEUE: u16 = 1;

/// Largest queue this driver's static rings can describe.
const MAX_QUEUE: usize = 256;
const QUEUE_ALIGN: usize = 4096;

/// The legacy `struct virtio_net_hdr` with no mergeable-buffer field.
const NET_HDR_LEN: usize = 10;
/// One receive/transmit buffer: header plus a full standard Ethernet frame.
const FRAME_CAP: usize = 1514;
const BUF_LEN: usize = NET_HDR_LEN + FRAME_CAP;
/// How many receive buffers to post. Posted once, up front: enough for the ARP
/// reply and then the ICMP echo reply, while tolerating any stray frame SLIRP
/// might emit first. Each proof consumes one; the rest stay available.
const RX_BUFFERS: usize = 8;

/// Ring storage: descriptor table + available ring + (aligned) used ring, sized
/// for `MAX_QUEUE`. Page aligned so its physical frame number is exact.
#[repr(C, align(4096))]
struct VRing([u8; 16384]);

static mut RX_RING: VRing = VRing([0; 16384]);
static mut TX_RING: VRing = VRing([0; 16384]);

#[repr(C, align(16))]
struct Buffer([u8; BUF_LEN]);

static mut RX_BUFS: [Buffer; RX_BUFFERS] = [const { Buffer([0; BUF_LEN]) }; RX_BUFFERS];
static mut TX_BUF: Buffer = Buffer([0; BUF_LEN]);

/// A brought-up virtio-net device.
#[derive(Clone, Copy)]
pub struct NetDevice {
    base: u16,
    mac: [u8; 6],
    rx_size: usize,
    tx_size: usize,
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

/// Point one queue's PFN at `ring`, after checking the device's negotiated size.
unsafe fn setup_queue(base: u16, queue: u16, ring_phys: u64) -> Option<usize> {
    // SAFETY: select the queue and read the size the device chose for it.
    let size = unsafe {
        outw(base + VIRTIO_QUEUE_SELECT, queue);
        inw(base + VIRTIO_QUEUE_SIZE) as usize
    };
    if size == 0 || size > MAX_QUEUE || !size.is_power_of_two() {
        return None;
    }
    // SAFETY: hand the page-aligned ring for this queue to the device.
    unsafe { outl(base + VIRTIO_QUEUE_PFN, (ring_phys >> 12) as u32) };
    Some(size)
}

/// Find a legacy virtio-net device and bring it up: handshake, both virtqueues,
/// and the MAC read from config space.
pub fn init() -> Option<NetDevice> {
    let location = find_virtio_net()?;
    debug_write("AW_VIRTIO_NET_FOUND bus=");
    debug_write_u64(u64::from(location.bus));
    debug_write(" device=");
    debug_write_u64(u64::from(location.device));
    debug_write(" function=");
    debug_write_u64(u64::from(location.function));
    debug_write("\n");
    let base = prepare(location)?;

    // Reset, acknowledge, claim, then negotiate only the MAC feature.
    // SAFETY: `base` is this device's I/O BAR; register offsets are fixed.
    unsafe {
        outb(base + VIRTIO_STATUS, 0);
        while inb(base + VIRTIO_STATUS) != 0 {
            core::hint::spin_loop();
        }
        outb(base + VIRTIO_STATUS, STATUS_ACKNOWLEDGE);
        outb(base + VIRTIO_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        let features = inl(base + VIRTIO_DEVICE_FEATURES);
        outl(base + VIRTIO_GUEST_FEATURES, features & VIRTIO_NET_F_MAC);
    }

    let rx_phys = core::ptr::addr_of!(RX_RING) as u64;
    let tx_phys = core::ptr::addr_of!(TX_RING) as u64;
    // SAFETY: both rings are page-aligned statics the identity map covers.
    let (rx_size, tx_size) = unsafe {
        let rx = setup_queue(base, RX_QUEUE, rx_phys);
        let tx = setup_queue(base, TX_QUEUE, tx_phys);
        match (rx, tx) {
            (Some(rx), Some(tx)) => (rx, tx),
            _ => {
                outb(base + VIRTIO_STATUS, STATUS_FAILED);
                return None;
            }
        }
    };

    // SAFETY: MAC is the first six bytes of device config; read it byte-wise so
    // the read does not depend on config-space word alignment.
    let mut mac = [0u8; 6];
    for (index, slot) in mac.iter_mut().enumerate() {
        *slot = unsafe { inb(base + VIRTIO_CONFIG + index as u16) };
    }

    // SAFETY: queues are configured; go live.
    unsafe {
        outb(
            base + VIRTIO_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_DRIVER_OK,
        );
    }

    Some(NetDevice {
        base,
        mac,
        rx_size,
        tx_size,
    })
}

impl NetDevice {
    /// Post every receive buffer into the receive ring and notify the device.
    ///
    /// Each buffer is a single device-writable descriptor. The available ring is
    /// filled once, up front: the device may complete them in any order, and this
    /// driver reads completions by scanning the used ring.
    fn post_receive_buffers(&self) {
        let ring = core::ptr::addr_of_mut!(RX_RING) as *mut u8;
        let avail = avail_offset(self.rx_size);
        // The index maps one-to-one onto a descriptor slot, an availability slot
        // and a receive buffer, so a range loop is exactly what is wanted here.
        #[allow(clippy::needless_range_loop)]
        for index in 0..RX_BUFFERS {
            // SAFETY: RX_BUFS[index] is a live static buffer, identity-mapped.
            let buf = unsafe { core::ptr::addr_of_mut!(RX_BUFS[index]) as u64 };
            // SAFETY: descriptor `index` is inside the ring (RX_BUFFERS < size).
            unsafe { write_desc(ring, index, buf, BUF_LEN as u32, VRING_DESC_WRITE, 0) };
            // SAFETY: the availability slot for this entry is inside the ring.
            unsafe { write_u16(ring, avail + 4 + index * 2, index as u16) };
        }
        compiler_fence(Ordering::SeqCst);
        // SAFETY: publish all posted buffers, then notify the receive queue.
        unsafe {
            write_u16(ring, avail, 0); // flags
            write_u16(ring, avail + 2, RX_BUFFERS as u16); // idx
            core::arch::asm!("mfence", options(nostack, preserves_flags));
            outw(self.base + VIRTIO_QUEUE_NOTIFY, RX_QUEUE);
        }
    }

    /// The device's used-ring index for the receive queue.
    fn rx_used_index(&self) -> u16 {
        let ring = core::ptr::addr_of!(RX_RING) as *mut u8;
        let used = used_offset(self.rx_size);
        compiler_fence(Ordering::SeqCst);
        // SAFETY: `used+2` is inside the ring.
        unsafe { read_u16(ring, used + 2) }
    }

    /// Read one completed receive entry: the descriptor id and byte length the
    /// device recorded in used-ring slot `slot`.
    fn rx_used_entry(&self, slot: u16) -> (u32, u32) {
        let ring = core::ptr::addr_of!(RX_RING) as *mut u8;
        let used = used_offset(self.rx_size);
        let base = used + 4 + (usize::from(slot) % self.rx_size) * 8;
        // SAFETY: the used element at `base` (id: u32, len: u32) is inside the ring.
        unsafe {
            let id = ring.add(base).cast::<u32>().read_volatile();
            let len = ring.add(base + 4).cast::<u32>().read_volatile();
            (id, len)
        }
    }

    /// Transmit one Ethernet frame already staged in `TX_BUF` after the 10-byte
    /// virtio-net header, and wait for the device to consume it.
    fn transmit(&self, frame_len: usize) -> Result<(), &'static str> {
        let ring = core::ptr::addr_of_mut!(TX_RING) as *mut u8;
        let tx = core::ptr::addr_of_mut!(TX_BUF) as *mut u8;

        // Zero the virtio-net header: no offloads, no GSO.
        // SAFETY: TX_BUF is at least NET_HDR_LEN bytes.
        unsafe {
            for offset in 0..NET_HDR_LEN {
                tx.add(offset).write_volatile(0);
            }
        }

        let total = NET_HDR_LEN + frame_len;
        // SAFETY: one device-readable descriptor spanning header+frame.
        unsafe { write_desc(ring, 0, tx as u64, total as u32, 0, 0) };

        let avail = avail_offset(self.tx_size);
        let used = used_offset(self.tx_size);
        // SAFETY: single outstanding transmit; publish descriptor 0.
        let used_before = unsafe {
            let before = read_u16(ring, used + 2);
            write_u16(ring, avail, 0);
            write_u16(ring, avail + 4, 0);
            compiler_fence(Ordering::SeqCst);
            let idx = read_u16(ring, avail + 2);
            write_u16(ring, avail + 2, idx.wrapping_add(1));
            before
        };

        // SAFETY: barrier then notify the transmit queue.
        unsafe {
            core::arch::asm!("mfence", options(nostack, preserves_flags));
            outw(self.base + VIRTIO_QUEUE_NOTIFY, TX_QUEUE);
        }

        let mut budget = 200_000_000u32;
        loop {
            // SAFETY: reading used.idx from the transmit ring.
            if unsafe { read_u16(ring, used + 2) } != used_before {
                return Ok(());
            }
            budget -= 1;
            if budget == 0 {
                return Err("tx_no_completion");
            }
            core::hint::spin_loop();
        }
    }
}

// ---- ARP proof ----------------------------------------------------------

/// SLIRP's fixed layout: the guest is 10.0.2.15 and the gateway is 10.0.2.2.
const GUEST_IP: [u8; 4] = [10, 0, 2, 15];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
const ETHERTYPE_ARP: u16 = 0x0806;

/// Build an ARP "who has GATEWAY_IP" request from `mac`/`GUEST_IP` into `TX_BUF`
/// just past the virtio-net header, and return its length in bytes.
fn build_arp_request(mac: &[u8; 6]) -> usize {
    let tx = core::ptr::addr_of_mut!(TX_BUF) as *mut u8;
    let mut frame = [0u8; 42];
    // Ethernet header: broadcast destination, our source, ARP ethertype.
    frame[0..6].copy_from_slice(&[0xff; 6]);
    frame[6..12].copy_from_slice(mac);
    frame[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    // ARP: Ethernet/IPv4, request.
    frame[14..16].copy_from_slice(&1u16.to_be_bytes()); // htype = Ethernet
    frame[16..18].copy_from_slice(&0x0800u16.to_be_bytes()); // ptype = IPv4
    frame[18] = 6; // hlen
    frame[19] = 4; // plen
    frame[20..22].copy_from_slice(&1u16.to_be_bytes()); // oper = request
    frame[22..28].copy_from_slice(mac); // sender hardware address
    frame[28..32].copy_from_slice(&GUEST_IP); // sender protocol address
    // target hardware address left zero
    frame[38..42].copy_from_slice(&GATEWAY_IP); // target protocol address

    // SAFETY: TX_BUF holds NET_HDR_LEN + FRAME_CAP >= NET_HDR_LEN + 42 bytes.
    unsafe {
        for (index, byte) in frame.iter().enumerate() {
            tx.add(NET_HDR_LEN + index).write_volatile(*byte);
        }
    }
    frame.len()
}

/// Scan the posted receive buffers for an ARP reply from the gateway, returning
/// its sender hardware address. Reads whatever completions are present now; the
/// caller sequences this after a transmit and a bounded wait.
fn find_arp_reply(device: &NetDevice, used_now: u16, used_before: u16) -> Option<[u8; 6]> {
    let mut slot = used_before;
    while slot != used_now {
        let (id, len) = device.rx_used_entry(slot);
        slot = slot.wrapping_add(1);
        let index = id as usize;
        if index >= RX_BUFFERS || (len as usize) < NET_HDR_LEN + 42 {
            continue;
        }
        // SAFETY: RX_BUFS[index] was written by the device up to `len` bytes.
        let buf = unsafe { core::ptr::addr_of!(RX_BUFS[index]) as *const u8 };
        let mut frame = [0u8; 42];
        // SAFETY: reading the 42 ARP bytes past the 10-byte net header, all
        // within the `len` the device reported and inside the buffer.
        unsafe {
            for (offset, byte) in frame.iter_mut().enumerate() {
                *byte = buf.add(NET_HDR_LEN + offset).read_volatile();
            }
        }
        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        let oper = u16::from_be_bytes([frame[20], frame[21]]);
        let sender_ip = [frame[28], frame[29], frame[30], frame[31]];
        if ethertype == ETHERTYPE_ARP && oper == 2 && sender_ip == GATEWAY_IP {
            return Some([
                frame[22], frame[23], frame[24], frame[25], frame[26], frame[27],
            ]);
        }
    }
    None
}

// ---- ICMP echo proof ----------------------------------------------------
//
// One layer up from ARP: a routed IPv4 datagram carrying an ICMP echo request
// to the gateway (10.0.2.2), addressed on the wire to the hardware address ARP
// resolved. The reply must echo our exact identifier, sequence and payload, so a
// match can only be the gateway echoing the request we sent.

/// Bytes of the distinctive echo payload. Its presence, verbatim, in the reply
/// is what makes the reply attributable to our request and not ambient traffic.
const ICMP_PAYLOAD_LEN: usize = 16;
const ICMP_PAYLOAD: [u8; ICMP_PAYLOAD_LEN] = *b"AW-ICMP-PROOF-01";
/// Our echo identifier and sequence, carried through and matched in the reply.
const ICMP_IDENT: u16 = 0xab01;
const ICMP_SEQ: u16 = 1;
const ETHERTYPE_IPV4: u16 = 0x0800;
const IP_PROTO_ICMP: u8 = 1;
/// IPv4 header length with no options, and the full ICMP echo length.
const IP_HDR_LEN: usize = 20;
const ICMP_LEN: usize = 8 + ICMP_PAYLOAD_LEN;

/// Partial one's-complement sum of `data` as big-endian 16-bit words, a trailing
/// odd byte padded with a zero low half. Returned unfolded so several regions
/// (e.g. a UDP pseudo-header and its payload) can be accumulated before folding.
fn checksum_sum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let (pairs, remainder) = data.as_chunks::<2>();
    for pair in pairs {
        sum += u32::from(u16::from_be_bytes(*pair));
    }
    if let [last] = remainder {
        sum += u32::from(u16::from_be_bytes([*last, 0]));
    }
    sum
}

/// Fold a partial sum into the final 16-bit one's-complement checksum.
fn checksum_fold(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// The standard Internet checksum (RFC 1071) over one contiguous region.
fn internet_checksum(data: &[u8]) -> u16 {
    checksum_fold(checksum_sum(data))
}

/// Build an Ethernet + IPv4 + ICMP echo request to the gateway into `TX_BUF` just
/// past the virtio-net header, and return its total length in bytes.
fn build_icmp_echo(mac: &[u8; 6], gateway_mac: &[u8; 6]) -> usize {
    const FRAME_LEN: usize = 14 + IP_HDR_LEN + ICMP_LEN;
    let mut frame = [0u8; FRAME_LEN];

    // Ethernet header: to the resolved gateway, from us, IPv4.
    frame[0..6].copy_from_slice(gateway_mac);
    frame[6..12].copy_from_slice(mac);
    frame[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

    // ICMP echo request. Built before the IP header so its checksum is settled
    // before the IP total-length/checksum are computed over a stable payload.
    {
        let icmp = &mut frame[14 + IP_HDR_LEN..];
        // Checksum field icmp[2..4] is left zero while it is summed over.
        icmp[0] = 8; // type = echo request
        icmp[1] = 0; // code
        icmp[4..6].copy_from_slice(&ICMP_IDENT.to_be_bytes());
        icmp[6..8].copy_from_slice(&ICMP_SEQ.to_be_bytes());
        icmp[8..8 + ICMP_PAYLOAD_LEN].copy_from_slice(&ICMP_PAYLOAD);
    }
    let icmp_ck = internet_checksum(&frame[14 + IP_HDR_LEN..14 + IP_HDR_LEN + ICMP_LEN]);
    frame[14 + IP_HDR_LEN + 2..14 + IP_HDR_LEN + 4].copy_from_slice(&icmp_ck.to_be_bytes());

    // IPv4 header.
    {
        let ip = &mut frame[14..14 + IP_HDR_LEN];
        // Header checksum field ip[10..12] is left zero while it is summed over.
        ip[0] = 0x45; // version 4, IHL 5 (no options)
        ip[1] = 0; // DSCP/ECN
        ip[2..4].copy_from_slice(&((IP_HDR_LEN + ICMP_LEN) as u16).to_be_bytes());
        ip[4..6].copy_from_slice(&0x0000u16.to_be_bytes()); // identification
        ip[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // flags = DF, offset 0
        ip[8] = 64; // TTL
        ip[9] = IP_PROTO_ICMP;
        ip[12..16].copy_from_slice(&GUEST_IP);
        ip[16..20].copy_from_slice(&GATEWAY_IP);
    }
    let ip_ck = internet_checksum(&frame[14..14 + IP_HDR_LEN]);
    frame[14 + 10..14 + 12].copy_from_slice(&ip_ck.to_be_bytes());

    // Stage into TX_BUF past the virtio-net header.
    let tx = core::ptr::addr_of_mut!(TX_BUF) as *mut u8;
    // SAFETY: TX_BUF holds NET_HDR_LEN + FRAME_CAP >= NET_HDR_LEN + FRAME_LEN.
    unsafe {
        for (index, byte) in frame.iter().enumerate() {
            tx.add(NET_HDR_LEN + index).write_volatile(*byte);
        }
    }
    frame.len()
}

/// Scan the posted receive buffers for the gateway's ICMP echo reply: an IPv4
/// datagram from 10.0.2.2 to us carrying an ICMP echo reply with our identifier,
/// sequence and payload. Returns true on the first exact match.
fn find_icmp_reply(device: &NetDevice, used_now: u16, used_before: u16) -> bool {
    const NEED: usize = 14 + IP_HDR_LEN + ICMP_LEN;
    let mut slot = used_before;
    while slot != used_now {
        let (id, len) = device.rx_used_entry(slot);
        slot = slot.wrapping_add(1);
        let index = id as usize;
        if index >= RX_BUFFERS || (len as usize) < NET_HDR_LEN + NEED {
            continue;
        }
        // SAFETY: RX_BUFS[index] was written by the device up to `len` bytes.
        let buf = unsafe { core::ptr::addr_of!(RX_BUFS[index]) as *const u8 };
        let mut frame = [0u8; NEED];
        // SAFETY: reading NEED bytes past the 10-byte net header, within `len`.
        unsafe {
            for (offset, byte) in frame.iter_mut().enumerate() {
                *byte = buf.add(NET_HDR_LEN + offset).read_volatile();
            }
        }
        if u16::from_be_bytes([frame[12], frame[13]]) != ETHERTYPE_IPV4 {
            continue;
        }
        let ihl = usize::from(frame[14] & 0x0f) * 4;
        if frame[14] >> 4 != 4 || ihl != IP_HDR_LEN || frame[14 + 9] != IP_PROTO_ICMP {
            continue;
        }
        let src = [frame[14 + 12], frame[14 + 13], frame[14 + 14], frame[14 + 15]];
        let dst = [frame[14 + 16], frame[14 + 17], frame[14 + 18], frame[14 + 19]];
        if src != GATEWAY_IP || dst != GUEST_IP {
            continue;
        }
        let icmp = &frame[14 + IP_HDR_LEN..];
        let ident = u16::from_be_bytes([icmp[4], icmp[5]]);
        let seq = u16::from_be_bytes([icmp[6], icmp[7]]);
        if icmp[0] == 0 // echo reply
            && icmp[1] == 0
            && ident == ICMP_IDENT
            && seq == ICMP_SEQ
            && icmp[8..8 + ICMP_PAYLOAD_LEN] == ICMP_PAYLOAD
        {
            return true;
        }
    }
    false
}

/// Send one ICMP echo to the gateway and match its reply. The ARP proof already
/// posted the receive buffers and resolved the gateway hardware address; this
/// reuses both. Attribution is the exact identifier/sequence/payload echoed back,
/// so no separate quiet window is needed here.
fn prove_icmp(device: &NetDevice, gateway_mac: &[u8; 6]) {
    let used_before = device.rx_used_index();
    let frame_len = build_icmp_echo(&device.mac, gateway_mac);
    if let Err(reason) = device.transmit(frame_len) {
        debug_write("AW_VIRTIO_NET_ICMP_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_VIRTIO_NET_ICMP_SENT\n");

    let mut budget = 200_000_000u32;
    let matched = loop {
        let used_now = device.rx_used_index();
        if used_now != used_before && find_icmp_reply(device, used_now, used_before) {
            break true;
        }
        budget -= 1;
        if budget == 0 {
            break false;
        }
        core::hint::spin_loop();
    };

    if matched {
        debug_write("AW_VIRTIO_NET_ICMP_REPLY_OK src=10.0.2.2 id=ab01 seq=1\n");
        debug_write("AW_VIRTIO_NET_ICMP_PROOF_OK\n");
    } else {
        debug_write("AW_VIRTIO_NET_ICMP_FAIL reason=no_echo_reply\n");
    }
}

// ---- DHCP (UDP) proof ---------------------------------------------------
//
// ICMP proved a routed IPv4 round-trip; UDP is the next layer up. With no
// address yet, the guest broadcasts a DHCPDISCOVER to SLIRP's built-in DHCP
// server (UDP port 67 at 10.0.2.2) and matches its DHCPOFFER. The offer is
// attributable by the 32-bit transaction id we chose, echoed back verbatim, and
// it carries the address SLIRP leases us (yiaddr 10.0.2.15) - a real UDP
// application exchange, checksum included, not a status bit.

const IP_PROTO_UDP: u8 = 17;
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_MAGIC: u32 = 0x6382_5363;
/// Our transaction id: distinctive so the offer that echoes it is unmistakably
/// the answer to our discover ("AW" then 0x0d01).
const DHCP_XID: u32 = 0x4157_0d01;
const DHCP_MSG_DISCOVER: u8 = 1;
const DHCP_MSG_OFFER: u8 = 2;
const DHCP_OPT_MSG_TYPE: u8 = 53;
const DHCP_OPT_PARAM_LIST: u8 = 55;
const DHCP_OPT_END: u8 = 255;
const DHCP_OPT_PAD: u8 = 0;
/// Offsets inside the frame (Ethernet, IPv4, UDP, BOOTP).
const UDP_OFF: usize = 14 + IP_HDR_LEN; // 34
const BOOTP_OFF: usize = UDP_OFF + 8; // 42
const BOOTP_XID_OFF: usize = BOOTP_OFF + 4; // 46
const BOOTP_YIADDR_OFF: usize = BOOTP_OFF + 16; // 58
const BOOTP_CHADDR_OFF: usize = BOOTP_OFF + 28; // 70
const BOOTP_MAGIC_OFF: usize = BOOTP_OFF + 236; // 278
const DHCP_OPTIONS_OFF: usize = BOOTP_MAGIC_OFF + 4; // 282

/// The Internet-broadcast IPv4 and hardware addresses.
const BROADCAST_IP: [u8; 4] = [255, 255, 255, 255];
const BROADCAST_MAC: [u8; 6] = [0xff; 6];

/// UDP checksum over IPv4: the Internet checksum of the pseudo-header (source and
/// destination address, a zero byte, the protocol, and the UDP length) followed
/// by the UDP header and data. A computed zero is sent as 0xffff (RFC 768).
fn udp_checksum(src: &[u8; 4], dst: &[u8; 4], udp: &[u8]) -> u16 {
    let mut sum = checksum_sum(src) + checksum_sum(dst);
    sum += u32::from(IP_PROTO_UDP); // zero byte (high) + protocol (low)
    sum += udp.len() as u32; // UDP length in the pseudo-header
    sum += checksum_sum(udp); // the UDP header (checksum field zero) and data
    let folded = checksum_fold(sum);
    if folded == 0 { 0xffff } else { folded }
}

/// Build a broadcast DHCPDISCOVER into `TX_BUF` just past the virtio-net header,
/// and return its total length in bytes.
fn build_dhcp_discover(mac: &[u8; 6]) -> usize {
    // BOOTP fixed area (236) + magic cookie (4) + options.
    // Options: message-type=DISCOVER, a parameter request list, then end.
    const OPTIONS: [u8; 9] = [
        DHCP_OPT_MSG_TYPE,
        1,
        DHCP_MSG_DISCOVER,
        DHCP_OPT_PARAM_LIST,
        3,
        1, // subnet mask
        3, // router
        6, // DNS
        DHCP_OPT_END,
    ];
    const FRAME_LEN: usize = DHCP_OPTIONS_OFF + OPTIONS.len();
    const UDP_LEN: usize = FRAME_LEN - UDP_OFF;
    const IP_TOTAL: usize = FRAME_LEN - 14;
    let mut frame = [0u8; FRAME_LEN];

    // Ethernet: broadcast, from us, IPv4.
    frame[0..6].copy_from_slice(&BROADCAST_MAC);
    frame[6..12].copy_from_slice(mac);
    frame[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

    // BOOTP request.
    frame[BOOTP_OFF] = 1; // op = BOOTREQUEST
    frame[BOOTP_OFF + 1] = 1; // htype = Ethernet
    frame[BOOTP_OFF + 2] = 6; // hlen
    frame[BOOTP_XID_OFF..BOOTP_XID_OFF + 4].copy_from_slice(&DHCP_XID.to_be_bytes());
    // flags: broadcast, so the server replies to the broadcast address (we have
    // no unicast IP to receive at yet).
    frame[BOOTP_OFF + 10..BOOTP_OFF + 12].copy_from_slice(&0x8000u16.to_be_bytes());
    frame[BOOTP_CHADDR_OFF..BOOTP_CHADDR_OFF + 6].copy_from_slice(mac);
    frame[BOOTP_MAGIC_OFF..BOOTP_MAGIC_OFF + 4].copy_from_slice(&DHCP_MAGIC.to_be_bytes());
    frame[DHCP_OPTIONS_OFF..FRAME_LEN].copy_from_slice(&OPTIONS);

    // UDP header (checksum field left zero while it is summed over).
    frame[UDP_OFF..UDP_OFF + 2].copy_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());
    frame[UDP_OFF + 2..UDP_OFF + 4].copy_from_slice(&DHCP_SERVER_PORT.to_be_bytes());
    frame[UDP_OFF + 4..UDP_OFF + 6].copy_from_slice(&(UDP_LEN as u16).to_be_bytes());
    let udp_ck = udp_checksum(&[0, 0, 0, 0], &BROADCAST_IP, &frame[UDP_OFF..FRAME_LEN]);
    frame[UDP_OFF + 6..UDP_OFF + 8].copy_from_slice(&udp_ck.to_be_bytes());

    // IPv4 header (checksum field left zero while it is summed over).
    frame[14] = 0x45; // version 4, IHL 5
    frame[16..18].copy_from_slice(&(IP_TOTAL as u16).to_be_bytes());
    frame[22] = 64; // TTL
    frame[23] = IP_PROTO_UDP;
    // source 0.0.0.0 stays zero; destination is the broadcast address.
    frame[30..34].copy_from_slice(&BROADCAST_IP);
    let ip_ck = internet_checksum(&frame[14..14 + IP_HDR_LEN]);
    frame[24..26].copy_from_slice(&ip_ck.to_be_bytes());

    let tx = core::ptr::addr_of_mut!(TX_BUF) as *mut u8;
    // SAFETY: TX_BUF holds NET_HDR_LEN + FRAME_CAP >= NET_HDR_LEN + FRAME_LEN.
    unsafe {
        for (index, byte) in frame.iter().enumerate() {
            tx.add(NET_HDR_LEN + index).write_volatile(*byte);
        }
    }
    frame.len()
}

/// Walk the DHCP options in `frame[..avail]` and return the message-type value
/// (option 53), if present and in bounds.
fn dhcp_message_type(frame: &[u8], avail: usize) -> Option<u8> {
    let mut index = DHCP_OPTIONS_OFF;
    while index < avail {
        let option = frame[index];
        if option == DHCP_OPT_END {
            break;
        }
        if option == DHCP_OPT_PAD {
            index += 1;
            continue;
        }
        if index + 2 > avail {
            break;
        }
        let length = usize::from(frame[index + 1]);
        if index + 2 + length > avail {
            break;
        }
        if option == DHCP_OPT_MSG_TYPE && length >= 1 {
            return Some(frame[index + 2]);
        }
        index += 2 + length;
    }
    None
}

/// Scan the posted receive buffers for the DHCPOFFER: a UDP datagram from the
/// server (port 67) to the client port carrying our transaction id, the magic
/// cookie and message-type OFFER. Returns the offered address (yiaddr).
fn find_dhcp_offer(device: &NetDevice, used_now: u16, used_before: u16) -> Option<[u8; 4]> {
    /// How much of each receive buffer to inspect: enough for the BOOTP fixed
    /// area, the magic cookie and the server's options.
    const SCAN: usize = 384;
    let mut slot = used_before;
    while slot != used_now {
        let (id, len) = device.rx_used_entry(slot);
        slot = slot.wrapping_add(1);
        let index = id as usize;
        let frame_len = (len as usize).saturating_sub(NET_HDR_LEN);
        if index >= RX_BUFFERS || frame_len <= DHCP_OPTIONS_OFF {
            continue;
        }
        let avail = frame_len.min(SCAN);
        // SAFETY: RX_BUFS[index] was written by the device up to `len` bytes.
        let buf = unsafe { core::ptr::addr_of!(RX_BUFS[index]) as *const u8 };
        let mut frame = [0u8; SCAN];
        // SAFETY: reading `avail` bytes past the 10-byte net header, within `len`.
        unsafe {
            for (offset, byte) in frame[..avail].iter_mut().enumerate() {
                *byte = buf.add(NET_HDR_LEN + offset).read_volatile();
            }
        }
        if u16::from_be_bytes([frame[12], frame[13]]) != ETHERTYPE_IPV4 {
            continue;
        }
        let ihl = usize::from(frame[14] & 0x0f) * 4;
        if frame[14] >> 4 != 4 || ihl != IP_HDR_LEN || frame[14 + 9] != IP_PROTO_UDP {
            continue;
        }
        let src_port = u16::from_be_bytes([frame[UDP_OFF], frame[UDP_OFF + 1]]);
        let dst_port = u16::from_be_bytes([frame[UDP_OFF + 2], frame[UDP_OFF + 3]]);
        if src_port != DHCP_SERVER_PORT || dst_port != DHCP_CLIENT_PORT {
            continue;
        }
        let xid = u32::from_be_bytes([
            frame[BOOTP_XID_OFF],
            frame[BOOTP_XID_OFF + 1],
            frame[BOOTP_XID_OFF + 2],
            frame[BOOTP_XID_OFF + 3],
        ]);
        let magic = u32::from_be_bytes([
            frame[BOOTP_MAGIC_OFF],
            frame[BOOTP_MAGIC_OFF + 1],
            frame[BOOTP_MAGIC_OFF + 2],
            frame[BOOTP_MAGIC_OFF + 3],
        ]);
        if frame[BOOTP_OFF] != 2 || xid != DHCP_XID || magic != DHCP_MAGIC {
            continue;
        }
        if dhcp_message_type(&frame, avail) != Some(DHCP_MSG_OFFER) {
            continue;
        }
        return Some([
            frame[BOOTP_YIADDR_OFF],
            frame[BOOTP_YIADDR_OFF + 1],
            frame[BOOTP_YIADDR_OFF + 2],
            frame[BOOTP_YIADDR_OFF + 3],
        ]);
    }
    None
}

/// Emit an IPv4 address as dotted decimal.
fn write_ipv4(addr: &[u8; 4]) {
    for (index, octet) in addr.iter().enumerate() {
        if index != 0 {
            debug_write(".");
        }
        debug_write_u64(u64::from(*octet));
    }
}

/// Broadcast a DHCPDISCOVER and match the server's DHCPOFFER. The receive buffers
/// the ARP proof posted are reused; attribution is the transaction id echoed
/// back, so no separate quiet window is needed here.
fn prove_dhcp(device: &NetDevice) {
    let used_before = device.rx_used_index();
    let frame_len = build_dhcp_discover(&device.mac);
    if let Err(reason) = device.transmit(frame_len) {
        debug_write("AW_VIRTIO_NET_DHCP_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_VIRTIO_NET_DHCP_DISCOVER_SENT\n");

    let mut budget = 200_000_000u32;
    let offer = loop {
        let used_now = device.rx_used_index();
        if used_now != used_before
            && let Some(yiaddr) = find_dhcp_offer(device, used_now, used_before)
        {
            break Some(yiaddr);
        }
        budget -= 1;
        if budget == 0 {
            break None;
        }
        core::hint::spin_loop();
    };

    match offer {
        Some(yiaddr) => {
            debug_write("AW_VIRTIO_NET_DHCP_OFFER_OK yiaddr=");
            write_ipv4(&yiaddr);
            debug_write("\n");
            if yiaddr == GUEST_IP {
                debug_write("AW_VIRTIO_NET_DHCP_PROOF_OK\n");
            } else {
                debug_write("AW_VIRTIO_NET_DHCP_FAIL reason=unexpected_yiaddr\n");
            }
        }
        None => debug_write("AW_VIRTIO_NET_DHCP_FAIL reason=no_offer\n"),
    }
}

/// Emit a MAC as `xx:xx:xx:xx:xx:xx`, two lowercase hex digits per byte, with no
/// `0x` prefix - the form the proof marker matches against.
fn write_mac(mac: &[u8; 6]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (index, byte) in mac.iter().enumerate() {
        if index != 0 {
            debug_write(":");
        }
        let pair = [HEX[usize::from(*byte >> 4)], HEX[usize::from(*byte & 0x0f)]];
        // SAFETY: both are ASCII hex digits, valid single-byte UTF-8.
        debug_write(unsafe { core::str::from_utf8_unchecked(&pair) });
    }
}

/// Prove a real transmit and receive: post receive buffers, confirm the ring is
/// quiet while nothing is sent, send an ARP request, and match the gateway's
/// reply. Prints `AW_VIRTIO_NET_UNAVAILABLE` and returns when no device is
/// present, so this is safe to call on every boot configuration.
pub fn prove() {
    let Some(device) = init() else {
        debug_write("AW_VIRTIO_NET_UNAVAILABLE reason=no_device\n");
        return;
    };

    debug_write("AW_VIRTIO_NET_MAC mac=");
    write_mac(&device.mac);
    debug_write("\n");

    device.post_receive_buffers();

    // Negative window: with buffers posted but nothing transmitted, no frame
    // should complete. This is what makes the later arrival attributable to our
    // own request rather than to ambient traffic (dossier section 6.1).
    let quiet_start = device.rx_used_index();
    let mut quiet_budget = 20_000_000u32;
    let mut disturbed = false;
    while quiet_budget != 0 {
        if device.rx_used_index() != quiet_start {
            disturbed = true;
            break;
        }
        quiet_budget -= 1;
        core::hint::spin_loop();
    }
    if disturbed {
        debug_write("AW_VIRTIO_NET_FAIL reason=unsolicited_receive\n");
        return;
    }
    debug_write("AW_VIRTIO_NET_QUIET_OK\n");

    let used_before = device.rx_used_index();
    let frame_len = build_arp_request(&device.mac);
    if let Err(reason) = device.transmit(frame_len) {
        debug_write("AW_VIRTIO_NET_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_VIRTIO_NET_ARP_SENT\n");

    // Wait, bounded, for the reply to land in the receive ring.
    let mut budget = 200_000_000u32;
    let reply = loop {
        let used_now = device.rx_used_index();
        if used_now != used_before
            && let Some(mac) = find_arp_reply(&device, used_now, used_before)
        {
            break Some(mac);
        }
        budget -= 1;
        if budget == 0 {
            break None;
        }
        core::hint::spin_loop();
    };

    let gateway_mac = match reply {
        Some(mac) => {
            debug_write("AW_VIRTIO_NET_ARP_REPLY_OK spa=10.0.2.2 sha=");
            write_mac(&mac);
            debug_write("\n");
            debug_write("AW_VIRTIO_NET_PROOF_OK\n");
            mac
        }
        None => {
            debug_write("AW_VIRTIO_NET_FAIL reason=no_arp_reply\n");
            return;
        }
    };

    // One layer up: a routed IPv4 + ICMP echo to the gateway ARP just resolved.
    prove_icmp(&device, &gateway_mac);
    // One layer up again: a UDP DHCP exchange with SLIRP's built-in server.
    prove_dhcp(&device);
}
