//! Kernel-side xHCI PCI discovery.
//!
//! This stage is deliberately non-destructive: it identifies the controller and
//! its BAR but does not reset it or touch MMIO yet. The next stage can therefore
//! build mapping/ownership rules from concrete controller data instead of
//! guessing an address or taking over an unrelated USB controller.

use aw_kernel_core::{HANDOFF_FLAG_PCIE_ECAM_PRESENT, KernelHandoff};

use crate::debug_write;
use crate::debug_write_hex_u64;
use crate::debug_write_u8;
use crate::pci_config::{
    BAR0, COMMAND_BUS_MASTER, COMMAND_MEMORY_SPACE, COMMAND_REGISTER, PciFunction,
};

const PCI_CLASS_SERIAL_BUS: u8 = 0x0c;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROGIF_XHCI: u8 = 0x30;
/// Conservative identity-mapped window for xHCI capabilities, operational
/// registers, ports, runtime registers and doorbells. Mapping does not access
/// the whole span; it only makes future controller registers reachable.
pub const MMIO_WINDOW_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Controller {
    pub function: PciFunction,
    pub bar0: u64,
    pub bar0_is_64: bool,
    pub command: u16,
}

fn is_xhci(class_revision: u32) -> bool {
    (class_revision >> 24) as u8 == PCI_CLASS_SERIAL_BUS
        && (class_revision >> 16) as u8 == PCI_SUBCLASS_USB
        && (class_revision >> 8) as u8 == PCI_PROGIF_XHCI
}

fn decode_bar0(function: PciFunction) -> Option<(u64, bool)> {
    let low = function.read_u32(BAR0)?;
    if low & 1 != 0 {
        return None;
    }

    let memory_type = (low >> 1) & 0x3;
    let low_base = u64::from(low & 0xffff_fff0);
    match memory_type {
        0x0 => (low_base != 0).then_some((low_base, false)),
        0x2 => {
            let high = function.read_u32(BAR0 + 4)?;
            let base = low_base | (u64::from(high) << 32);
            (base != 0).then_some((base, true))
        }
        _ => None,
    }
}

/// Find the first xHCI function described by the firmware-provided ECAM map.
///
/// No PCI configuration writes and no xHCI MMIO accesses occur here.
#[must_use]
pub fn find(handoff: &KernelHandoff) -> Option<Controller> {
    if handoff.flags & HANDOFF_FLAG_PCIE_ECAM_PRESENT == 0 {
        return None;
    }

    for region in handoff
        .pcie_ecam
        .iter()
        .take(handoff.pcie_ecam_count as usize)
        .copied()
    {
        for bus in region.start_bus..=region.end_bus {
            for device in 0u8..32 {
                let function0 = PciFunction::new(region, bus, device, 0)?;
                let vendor0 = function0.read_u16(0x00)?;
                if vendor0 == 0xffff {
                    continue;
                }
                let header = function0.read_u8(0x0e).unwrap_or(0);
                let count = if header & 0x80 != 0 { 8 } else { 1 };

                for function_number in 0u8..count {
                    let Some(function) = PciFunction::new(region, bus, device, function_number)
                    else {
                        continue;
                    };
                    let Some(vendor) = function.read_u16(0x00) else {
                        continue;
                    };
                    if vendor == 0xffff {
                        continue;
                    }
                    let Some(class_revision) = function.read_u32(0x08) else {
                        continue;
                    };
                    if !is_xhci(class_revision) {
                        continue;
                    }
                    let Some((bar0, bar0_is_64)) = decode_bar0(function) else {
                        continue;
                    };
                    let command = function.read_u16(COMMAND_REGISTER).unwrap_or(0);
                    return Some(Controller {
                        function,
                        bar0,
                        bar0_is_64,
                        command,
                    });
                }
            }
        }
    }
    None
}

/// Emit auditable xHCI PCI identity without taking controller ownership.
pub fn prove_pci_discovery(handoff: &KernelHandoff) {
    debug_write("AW_XHCI_PCI_BEGIN\n");
    let Some(controller) = find(handoff) else {
        debug_write("AW_XHCI_PCI_UNAVAILABLE\n");
        return;
    };

    debug_write("AW_XHCI_PCI_FOUND bus=");
    debug_write_u8(controller.function.bus());
    debug_write(" device=");
    debug_write_u8(controller.function.device());
    debug_write(" function=");
    debug_write_u8(controller.function.function());
    debug_write(" bar0=");
    debug_write_hex_u64(controller.bar0);
    debug_write(" bar64=");
    debug_write_u8(u8::from(controller.bar0_is_64));
    debug_write(" command=");
    debug_write_hex_u64(u64::from(controller.command));
    debug_write("\n");
    debug_write("AW_XHCI_PCI_DISCOVERY_PROOF_OK\n");
}

const CAP_HCSPARAMS1: u64 = 0x04;
const CAP_HCCPARAMS1: u64 = 0x10;
const CAP_DBOFF: u64 = 0x14;
const CAP_RTSOFF: u64 = 0x18;

unsafe fn mmio_read_u32(address: u64) -> u32 {
    // SAFETY: caller proves the address is inside the active low identity map
    // and belongs to the discovered xHCI BAR.
    unsafe { (address as *const u32).read_volatile() }
}

/// Read and validate xHCI capability registers without changing controller
/// state. This proves that the BAR is not only discoverable through PCI config
/// space but actually reachable through the kernel-owned page tables.
pub fn prove_mmio_capabilities(handoff: &KernelHandoff, memory_ready: bool) {
    debug_write("AW_XHCI_MMIO_BEGIN\n");
    if !memory_ready {
        debug_write("AW_XHCI_MMIO_UNAVAILABLE reason=vmm_not_ready\n");
        return;
    }
    let Some(controller) = find(handoff) else {
        debug_write("AW_XHCI_MMIO_UNAVAILABLE reason=no_controller\n");
        return;
    };

    let Some(last_register) = controller.bar0.checked_add(CAP_RTSOFF + 4) else {
        debug_write("AW_XHCI_MMIO_UNAVAILABLE reason=bar_overflow\n");
        return;
    };
    let Some(mapped_end) = controller.bar0.checked_add(MMIO_WINDOW_BYTES) else {
        debug_write("AW_XHCI_MMIO_UNAVAILABLE reason=window_overflow\n");
        return;
    };
    if last_register > mapped_end {
        debug_write("AW_XHCI_MMIO_UNAVAILABLE reason=window_too_small\n");
        return;
    }

    // SAFETY: activate_virtual_memory adds this controller's MMIO window to the
    // audited identity map before CR3 is switched, and BAR0 came from xHCI PCI.
    let cap0 = unsafe { mmio_read_u32(controller.bar0) };
    let cap_length = (cap0 & 0xff) as u8;
    let hcs1 = unsafe { mmio_read_u32(controller.bar0 + CAP_HCSPARAMS1) };
    let hcc1 = unsafe { mmio_read_u32(controller.bar0 + CAP_HCCPARAMS1) };
    let dboff = unsafe { mmio_read_u32(controller.bar0 + CAP_DBOFF) };
    let rtsoff = unsafe { mmio_read_u32(controller.bar0 + CAP_RTSOFF) };

    let max_slots = (hcs1 & 0xff) as u8;
    let max_ports = ((hcs1 >> 24) & 0xff) as u8;
    let context_bytes = if hcc1 & (1 << 2) != 0 { 64 } else { 32 };

    if cap_length < 0x20
        || max_slots == 0
        || max_ports == 0
        || dboff & 0x3 != 0
        || rtsoff & 0x1f != 0
    {
        debug_write("AW_XHCI_MMIO_FAIL reason=capability_shape\n");
        return;
    }

    debug_write("AW_XHCI_MMIO_CAP caplen=");
    debug_write_u8(cap_length);
    debug_write(" slots=");
    debug_write_u8(max_slots);
    debug_write(" ports=");
    debug_write_u8(max_ports);
    debug_write(" ctx=");
    debug_write_u8(context_bytes);
    debug_write(" dboff=");
    debug_write_hex_u64(u64::from(dboff));
    debug_write(" rtsoff=");
    debug_write_hex_u64(u64::from(rtsoff));
    debug_write("\n");
    debug_write("AW_XHCI_MMIO_CAP_PROOF_OK\n");
}

#[cfg(feature = "xhci-smoke-test")]
const CAP_HCSPARAMS2: u64 = 0x08;
#[cfg(feature = "xhci-smoke-test")]
const OP_USBCMD: u64 = 0x00;
#[cfg(feature = "xhci-smoke-test")]
const OP_USBSTS: u64 = 0x04;
#[cfg(feature = "xhci-smoke-test")]
const OP_CRCR: u64 = 0x18;
#[cfg(feature = "xhci-smoke-test")]
const OP_DCBAAP: u64 = 0x30;
#[cfg(feature = "xhci-smoke-test")]
const OP_CONFIG: u64 = 0x38;
#[cfg(feature = "xhci-smoke-test")]
const USBCMD_RS: u32 = 1 << 0;
#[cfg(feature = "xhci-smoke-test")]
const USBCMD_HCRST: u32 = 1 << 1;
#[cfg(feature = "xhci-smoke-test")]
const USBSTS_HCH: u32 = 1 << 0;
#[cfg(feature = "xhci-smoke-test")]
const USBSTS_HSE: u32 = 1 << 2;
#[cfg(feature = "xhci-smoke-test")]
const USBSTS_CNR: u32 = 1 << 11;
#[cfg(feature = "xhci-smoke-test")]
const WAIT_BUDGET: u32 = 100_000_000;
#[cfg(feature = "xhci-smoke-test")]
const COMMAND_RING_TRBS: usize = 64;
#[cfg(feature = "xhci-smoke-test")]
const EVENT_RING_TRBS: usize = 64;

#[cfg(feature = "xhci-smoke-test")]
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Trb {
    parameter: u64,
    status: u32,
    control: u32,
}

#[cfg(feature = "xhci-smoke-test")]
impl Trb {
    const ZERO: Self = Self {
        parameter: 0,
        status: 0,
        control: 0,
    };
}

#[cfg(feature = "xhci-smoke-test")]
#[repr(C, align(4096))]
struct DmaPage(#[allow(dead_code)] [u8; 4096]);

#[cfg(feature = "xhci-smoke-test")]
#[repr(C, align(4096))]
struct CommandRing(#[allow(dead_code)] [Trb; COMMAND_RING_TRBS]);

#[cfg(feature = "xhci-smoke-test")]
#[repr(C, align(4096))]
struct EventRing(#[allow(dead_code)] [Trb; EVENT_RING_TRBS]);

#[cfg(feature = "xhci-smoke-test")]
static mut SMOKE_DCBAA: DmaPage = DmaPage([0; 4096]);
#[cfg(feature = "xhci-smoke-test")]
static mut SMOKE_COMMAND_RING: CommandRing = CommandRing([Trb::ZERO; COMMAND_RING_TRBS]);
#[cfg(feature = "xhci-smoke-test")]
static mut SMOKE_EVENT_RING: EventRing = EventRing([Trb::ZERO; EVENT_RING_TRBS]);
#[cfg(feature = "xhci-smoke-test")]
static mut SMOKE_ERST: DmaPage = DmaPage([0; 4096]);

#[cfg(feature = "xhci-smoke-test")]
unsafe fn mmio_write_u32(address: u64, value: u32) {
    // SAFETY: caller owns the QEMU-only xHCI test controller and supplies an
    // audited MMIO address inside its mapped BAR window.
    unsafe { (address as *mut u32).write_volatile(value) };
}

#[cfg(feature = "xhci-smoke-test")]
unsafe fn mmio_write_u64(address: u64, value: u64) {
    // SAFETY: same ownership and mapping contract as mmio_write_u32.
    unsafe { (address as *mut u64).write_volatile(value) };
}

#[cfg(feature = "xhci-smoke-test")]
fn wait_mask(address: u64, mask: u32, expected: u32) -> bool {
    let mut budget = WAIT_BUDGET;
    while budget != 0 {
        // SAFETY: test-only caller mapped and owns the xHCI register window.
        if unsafe { mmio_read_u32(address) } & mask == expected {
            return true;
        }
        budget -= 1;
        core::hint::spin_loop();
    }
    false
}

#[cfg(feature = "xhci-smoke-test")]
fn range_inside_window(bar: u64, address: u64, bytes: u64) -> bool {
    let Some(window_end) = bar.checked_add(MMIO_WINDOW_BYTES) else {
        return false;
    };
    let Some(end) = address.checked_add(bytes) else {
        return false;
    };
    address >= bar && end <= window_end
}

/// Destructive xHCI ownership proof for QEMU only.
///
/// This function is impossible to include in a normal kernel unless the
/// dedicated `xhci-smoke-test` feature is enabled. It halts and resets the
/// controller, installs minimal DMA structures, starts it, then stops it again.
/// The PCI command register is restored before return. It does not enumerate a
/// USB device or claim HID input yet.
#[cfg(feature = "xhci-smoke-test")]
pub fn prove_controller_smoke(handoff: &KernelHandoff, memory_ready: bool) {
    debug_write("AW_XHCI_SMOKE_BEGIN\n");
    if !memory_ready {
        debug_write("AW_XHCI_SMOKE_FAIL reason=vmm_not_ready\n");
        return;
    }

    let Some(controller) = find(handoff) else {
        debug_write("AW_XHCI_SMOKE_FAIL reason=no_controller\n");
        return;
    };

    // The QEMU fixture must already expose an enabled controller. Do not mutate
    // PCI command bits here: the proof owns xHCI operational state, not BAR/PCI
    // setup. A disabled fixture is a hard test failure.
    if controller.command & (COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER)
        != (COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER)
    {
        debug_write("AW_XHCI_SMOKE_FAIL reason=pci_command_disabled\n");
        return;
    }

    let cap0 = unsafe { mmio_read_u32(controller.bar0) };
    let cap_length = u64::from(cap0 & 0xff);
    let hcs1 = unsafe { mmio_read_u32(controller.bar0 + CAP_HCSPARAMS1) };
    let hcs2 = unsafe { mmio_read_u32(controller.bar0 + CAP_HCSPARAMS2) };
    let max_slots = hcs1 & 0xff;
    let rtsoff = u64::from(unsafe { mmio_read_u32(controller.bar0 + CAP_RTSOFF) } & !0x1f);
    let op = controller.bar0 + cap_length;
    let runtime = controller.bar0 + rtsoff;
    let ir0 = runtime + 0x20;

    if cap_length < 0x20
        || max_slots == 0
        || !range_inside_window(controller.bar0, op + OP_CONFIG, 4)
        || !range_inside_window(controller.bar0, ir0 + 0x20, 8)
    {
        debug_write("AW_XHCI_SMOKE_FAIL reason=register_window\n");
        return;
    }

    // QEMU currently advertises no scratchpad buffers. Refuse rather than
    // installing an incomplete DCBAA if that changes.
    let max_scratchpads = ((hcs2 >> 27) & 0x1f) | ((hcs2 >> 16) & 0x3e0);
    if max_scratchpads != 0 {
        debug_write("AW_XHCI_SMOKE_FAIL reason=scratchpads_required\n");
        return;
    }

    // Halt.
    let command = unsafe { mmio_read_u32(op + OP_USBCMD) };
    unsafe { mmio_write_u32(op + OP_USBCMD, command & !USBCMD_RS) };
    if !wait_mask(op + OP_USBSTS, USBSTS_HCH, USBSTS_HCH) {
        debug_write("AW_XHCI_SMOKE_FAIL reason=halt_timeout\n");
        return;
    }
    debug_write("AW_XHCI_SMOKE_HALTED\n");

    // Host-controller reset and Controller Not Ready clearance.
    unsafe { mmio_write_u32(op + OP_USBCMD, USBCMD_HCRST) };
    if !wait_mask(op + OP_USBCMD, USBCMD_HCRST, 0) || !wait_mask(op + OP_USBSTS, USBSTS_CNR, 0) {
        debug_write("AW_XHCI_SMOKE_FAIL reason=reset_timeout\n");
        return;
    }
    debug_write("AW_XHCI_SMOKE_RESET_OK\n");

    // Zero all DMA-visible structures before handing their addresses to xHCI.
    unsafe {
        core::ptr::write_bytes(core::ptr::addr_of_mut!(SMOKE_DCBAA).cast::<u8>(), 0, 4096);
        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(SMOKE_COMMAND_RING).cast::<u8>(),
            0,
            core::mem::size_of::<CommandRing>(),
        );
        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(SMOKE_EVENT_RING).cast::<u8>(),
            0,
            core::mem::size_of::<EventRing>(),
        );
        core::ptr::write_bytes(core::ptr::addr_of_mut!(SMOKE_ERST).cast::<u8>(), 0, 4096);
    }

    let dcbaa = core::ptr::addr_of!(SMOKE_DCBAA) as u64;
    let command_ring = core::ptr::addr_of!(SMOKE_COMMAND_RING) as u64;
    let event_ring = core::ptr::addr_of!(SMOKE_EVENT_RING) as u64;
    let erst = core::ptr::addr_of_mut!(SMOKE_ERST) as *mut u64;

    // Event Ring Segment Table entry 0: base, size, reserved.
    unsafe {
        *erst.add(0) = event_ring;
        *erst.add(1) = EVENT_RING_TRBS as u64;
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    unsafe {
        mmio_write_u32(op + OP_CONFIG, max_slots);
        mmio_write_u64(op + OP_DCBAAP, dcbaa);
        mmio_write_u64(op + OP_CRCR, command_ring | 1); // Ring Cycle State = 1.
        mmio_write_u32(ir0 + 0x08, 1); // ERSTSZ
        mmio_write_u64(ir0 + 0x10, core::ptr::addr_of!(SMOKE_ERST) as u64); // ERSTBA
        mmio_write_u64(ir0 + 0x18, event_ring | (1 << 3)); // ERDP, clear EHB
        mmio_write_u32(ir0, 0); // IMAN: polling only.
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    debug_write("AW_XHCI_SMOKE_DMA_READY\n");

    // Run, prove HCHalted clears and Host System Error stays clear.
    unsafe { mmio_write_u32(op + OP_USBCMD, USBCMD_RS) };
    if !wait_mask(op + OP_USBSTS, USBSTS_HCH, 0) {
        debug_write("AW_XHCI_SMOKE_FAIL reason=run_timeout\n");
        return;
    }
    if unsafe { mmio_read_u32(op + OP_USBSTS) } & USBSTS_HSE != 0 {
        debug_write("AW_XHCI_SMOKE_FAIL reason=host_system_error\n");
        return;
    }
    debug_write("AW_XHCI_SMOKE_RUNNING\n");

    // Stop again so the test does not leave an owned controller active while
    // the remainder of the kernel proof suite runs.
    unsafe { mmio_write_u32(op + OP_USBCMD, 0) };
    if !wait_mask(op + OP_USBSTS, USBSTS_HCH, USBSTS_HCH) {
        debug_write("AW_XHCI_SMOKE_FAIL reason=stop_timeout\n");
        return;
    }
    debug_write("AW_XHCI_SMOKE_STOPPED\n");

    debug_write("AW_XHCI_SMOKE_PROOF_OK\n");
}
