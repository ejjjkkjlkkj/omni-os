#![no_main]
#![no_std]
// The dedicated smoke-test images intentionally diverge inside `_start` before
// the normal boot continuation runs, leaving its helper functions unused in
// those builds only. The default (normal) build keeps full dead-code analysis.
#![cfg_attr(
    any(feature = "exception-smoke-test", feature = "double-fault-smoke-test"),
    allow(dead_code)
)]

extern crate alloc;

mod acpi;
mod admin;
mod ahci;
mod apic_timer;
mod boot_menu;
mod braille;
mod clock;
mod device_irq;
mod facts;
mod fat16;
mod firmware_runtime;
mod font;
mod frame_allocator;
mod framebuffer;
mod gpt;
mod hda;
mod heap;
#[cfg(feature = "disk-build-smoke-test")]
mod installer;
mod interrupt_stub;
mod interrupt_vectors;
mod interrupts;
mod ioapic;
mod ipc;
mod irq_proof;
mod legacy_pic;
mod local_apic;
mod memory_protection;
mod msi;
#[cfg(feature = "msi-proof-device")]
mod msi_proof;
mod nvme;
mod page_mapper;
mod pci_config;
mod percpu;
mod pit;
mod power;
mod ps2_keyboard;
mod ring3;
mod rtc;
mod scheduler;
mod screen_reader;
mod security_baseline;
mod serial;
mod smp;
mod speech;
mod usb_hid_keyboard;
mod virtio_blk;
mod virtio_net;
mod virtual_memory;
mod xhci;

use irq_proof::DeliveryProof;
use memory_protection::{ProofOutcome, ProtectionProof};

use aw_kernel_core::{
    HANDOFF_FLAG_FRAMEBUFFER_PRESENT, HANDOFF_FLAG_PCIE_ECAM_PRESENT, HandoffPixelFormat,
    KernelHandoff, MemoryDescriptorHandoff, UEFI_MEMORY_TYPE_CONVENTIONAL,
};
use aw_memory::{BootstrapPageAllocator, PhysicalRange};
use aw_pci::{PciAddress, PciDeviceIdentity};
use aw_x86_platform::{
    CpuAddressWidths, CpuFeatures, CpuIdentity, CpuSignature, CpuVendor, CpuidRegisters,
};
use core::arch::asm;
use core::arch::x86_64::__cpuid_count;
use core::panic::PanicInfo;

const DEBUG_PORT: u16 = 0x00e9;
const PCI_CONFIG_ADDRESS_PORT: u16 = 0x0cf8;
const ECAM_BUS_BYTES: u64 = 1 << 20;
const MAX_HANDOFF_MMIO_RANGES: usize = 8;
const PCI_CONFIG_DATA_PORT: u16 = 0x0cfc;

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    // SAFETY: The caller ensures the selected I/O port accepts byte writes.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline(always)]
unsafe fn outl(port: u16, value: u32) {
    // SAFETY: The caller ensures the selected I/O port accepts dword writes.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline(always)]
unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: The caller ensures the selected I/O port accepts dword reads.
    unsafe {
        asm!(
            "in eax, dx",
            in("dx") port,
            out("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Emit one diagnostic byte: to the 0xE9 debug port, and mirrored to the real
/// 16550 once serial::prove has proved it present. The mirror is a no-op until
/// then, so a machine with no COM1 (the QEMU proofs run `-serial none`) is
/// unaffected; on hardware and hypervisors without a 0xE9 port (e.g. VMware) it is
/// how every marker - strings and numbers alike - reaches a visible console.
fn debug_put(byte: u8) {
    // SAFETY: DEBUG_PORT is the conventional byte-wide QEMU/Bochs debug port.
    unsafe { outb(DEBUG_PORT, byte) };
    serial::mirror_byte(byte);
}

fn debug_write(message: &str) {
    for byte in message.bytes() {
        debug_put(byte);
    }
}

fn debug_write_u8(mut value: u8) {
    let mut digits = [0_u8; 3];
    let mut index = digits.len();

    if value == 0 {
        debug_put(b'0');
        return;
    }

    while value != 0 {
        index -= 1;
        digits[index] = b'0' + value % 10;
        value /= 10;
    }

    for byte in &digits[index..] {
        debug_put(*byte);
    }
}

fn debug_write_u64(mut value: u64) {
    let mut digits = [0_u8; 20];
    let mut index = digits.len();

    if value == 0 {
        debug_put(b'0');
        return;
    }

    while value != 0 {
        index -= 1;
        digits[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &digits[index..] {
        debug_put(*byte);
    }
}

fn debug_write_hex_u64(value: u64) {
    debug_write("0x");
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0x0f) as u8;
        let byte = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + (nibble - 10)
        };
        debug_put(byte);
    }
}
#[inline(always)]
fn halt_forever() -> ! {
    loop {
        // SAFETY: Interrupts are disabled before entering the kernel main path,
        // so HLT cannot dispatch into an uninitialized interrupt descriptor table.
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

#[inline(always)]
fn cpuid(leaf: u32, subleaf: u32) -> CpuidRegisters {
    let registers = __cpuid_count(leaf, subleaf);
    CpuidRegisters {
        eax: registers.eax,
        ebx: registers.ebx,
        ecx: registers.ecx,
        edx: registers.edx,
    }
}

fn detect_cpu() -> CpuIdentity {
    let leaf0 = cpuid(0, 0);
    let max_basic_leaf = leaf0.eax;
    let extended_leaf0 = cpuid(0x8000_0000, 0);
    let max_extended_leaf = extended_leaf0.eax;

    let leaf1 = if max_basic_leaf >= 1 {
        cpuid(1, 0)
    } else {
        CpuidRegisters::ZERO
    };
    let leaf7 = (max_basic_leaf >= 7).then(|| cpuid(7, 0));
    let extended_leaf1 = (max_extended_leaf >= 0x8000_0001).then(|| cpuid(0x8000_0001, 0));
    let extended_leaf7 = (max_extended_leaf >= 0x8000_0007).then(|| cpuid(0x8000_0007, 0));
    let extended_leaf8 = (max_extended_leaf >= 0x8000_0008).then(|| cpuid(0x8000_0008, 0));

    CpuIdentity {
        vendor: CpuVendor::from_leaf0(leaf0),
        signature: CpuSignature::from_leaf1_eax(leaf1.eax),
        max_basic_leaf,
        max_extended_leaf,
        address_widths: CpuAddressWidths::from_extended_leaf8(extended_leaf8),
        features: CpuFeatures::from_leaves(leaf1, leaf7, extended_leaf1, extended_leaf7),
    }
}

fn debug_cpu_vendor(vendor: CpuVendor) {
    debug_write("AW_CPU_VENDOR_OK vendor=");
    match vendor {
        CpuVendor::Amd => debug_write("amd"),
        CpuVendor::Intel => debug_write("intel"),
        CpuVendor::Other(_) => debug_write("other"),
    }
    debug_write("\n");
}

fn debug_cpu_address_widths(widths: CpuAddressWidths) {
    debug_write("AW_CPU_ADDRESS_WIDTH_OK physical=");
    debug_write_u8(widths.physical);
    debug_write(" linear=");
    debug_write_u8(widths.linear);
    debug_write("\n");
}

fn validate_cpu_baseline() {
    let cpu = detect_cpu();

    debug_cpu_vendor(cpu.vendor);

    if cpu.features.meets_boot_baseline() {
        debug_write("AW_CPU_BASELINE_OK apic=1 sse2=1 long_mode=1\n");
    } else {
        debug_write("AW_CPU_BASELINE_FAIL\n");
        halt_forever();
    }

    if cpu.is_supported_vendor() {
        debug_write("AW_CPU_VENDOR_SUPPORTED\n");
    } else {
        debug_write("AW_CPU_VENDOR_GENERIC_FALLBACK\n");
    }

    if cpu.features.nx {
        debug_write("AW_CPU_NX_OK\n");
    } else {
        debug_write("AW_CPU_NX_FAIL\n");
        halt_forever();
    }

    debug_cpu_address_widths(cpu.address_widths);
    if cpu.meets_paging_baseline() {
        debug_write("AW_PAGING_BASELINE_OK\n");
    } else {
        debug_write("AW_PAGING_BASELINE_FAIL\n");
        halt_forever();
    }

    if cpu.features.x2apic {
        debug_write("AW_CPU_X2APIC_AVAILABLE\n");
    } else {
        debug_write("AW_CPU_X2APIC_FALLBACK_APIC\n");
    }

    if cpu.features.invariant_tsc {
        debug_write("AW_CPU_INVARIANT_TSC_AVAILABLE\n");
    } else {
        debug_write("AW_CPU_TIMER_FALLBACK_REQUIRED\n");
    }
}

fn memory_map_descriptors(handoff: &KernelHandoff) -> Option<&[MemoryDescriptorHandoff]> {
    let map = handoff.memory_map;
    if !map.is_valid() || map.buffer_address > usize::MAX as u64 {
        return None;
    }

    // SAFETY: The ABI v5 loader reserves the normalized descriptor buffer as
    // LOADER_DATA before ExitBootServices and transfers control without freeing
    // it. `MemoryMapHandoff::is_valid` verifies alignment, descriptor size and
    // byte length before this slice is constructed.
    Some(unsafe {
        core::slice::from_raw_parts(
            map.buffer_address as usize as *const MemoryDescriptorHandoff,
            map.entry_count as usize,
        )
    })
}

fn validate_memory_map(handoff: &KernelHandoff) -> bool {
    let Some(descriptors) = memory_map_descriptors(handoff) else {
        debug_write("AW_MEMORY_MAP_VALIDATE_FAIL reason=shape\n");
        return false;
    };

    let mut conventional_pages = 0_u64;
    for descriptor in descriptors {
        if !descriptor.is_valid() {
            debug_write("AW_MEMORY_MAP_VALIDATE_FAIL reason=descriptor\n");
            return false;
        }
        if descriptor.memory_type == UEFI_MEMORY_TYPE_CONVENTIONAL {
            let Some(total) = conventional_pages.checked_add(descriptor.page_count) else {
                debug_write("AW_MEMORY_MAP_VALIDATE_FAIL reason=page_overflow\n");
                return false;
            };
            conventional_pages = total;
        }
    }

    if conventional_pages == 0 {
        debug_write("AW_MEMORY_MAP_VALIDATE_FAIL reason=no_conventional_memory\n");
        return false;
    }

    debug_write("AW_MEMORY_MAP_VALIDATE_OK\n");
    debug_write("AW_MEMORY_MAP_CONVENTIONAL_OK\n");
    facts::MEMORY_MIB.store(
        conventional_pages / 256,
        core::sync::atomic::Ordering::Release,
    );
    true
}

fn probe_bootstrap_page_allocator(handoff: &KernelHandoff) -> bool {
    let Some(descriptors) = memory_map_descriptors(handoff) else {
        debug_write("AW_BOOTSTRAP_PAGE_ALLOC_FAIL reason=memory_map\n");
        return false;
    };

    let kernel_image = handoff.kernel_image;
    let Some(kernel_end) = kernel_image.allocation_end_exclusive() else {
        debug_write("AW_KERNEL_RANGE_PROTECTED_FAIL reason=overflow\n");
        return false;
    };
    let Some(kernel_range) = PhysicalRange::new(kernel_image.physical_address, kernel_end) else {
        debug_write("AW_KERNEL_RANGE_PROTECTED_FAIL reason=shape\n");
        return false;
    };
    let protected_ranges = [kernel_range];

    let mut allocator =
        match BootstrapPageAllocator::with_protected_ranges(descriptors, &protected_ranges) {
            Ok(allocator) => allocator,
            Err(_) => {
                debug_write("AW_BOOTSTRAP_PAGE_ALLOC_FAIL reason=allocator_init\n");
                return false;
            }
        };

    let Some(page) = allocator.allocate_page() else {
        debug_write("AW_BOOTSTRAP_PAGE_ALLOC_FAIL reason=no_page\n");
        return false;
    };
    if kernel_range.contains_address(page.start_address()) {
        debug_write("AW_KERNEL_RANGE_PROTECTED_FAIL reason=allocated_kernel_page\n");
        return false;
    }

    debug_write("AW_KERNEL_RANGE_PROTECTED_OK\n");
    debug_write("AW_BOOTSTRAP_PAGE_ALLOC_OK\n");
    true
}

fn activate_virtual_memory(handoff: &KernelHandoff) -> Option<virtual_memory::ActiveMap> {
    debug_write("AW_VMM_BEGIN\n");

    let map_handoff = handoff.memory_map;
    if !map_handoff.is_valid() || map_handoff.buffer_address > usize::MAX as u64 {
        debug_write("AW_VMM_FAIL reason=memory_map\n");
        return None;
    }
    // SAFETY: the ABI v5 loader reserves the normalized descriptor buffer as
    // LOADER_DATA before ExitBootServices and never frees it, so it is valid for
    // the remainder of the kernel's life - hence a `'static` slice. Shape was
    // validated by `is_valid` above.
    let descriptors: &'static [MemoryDescriptorHandoff] = unsafe {
        core::slice::from_raw_parts(
            map_handoff.buffer_address as usize as *const MemoryDescriptorHandoff,
            map_handoff.entry_count as usize,
        )
    };

    let kernel_image = handoff.kernel_image;
    let Some(kernel_end) = kernel_image.allocation_end_exclusive() else {
        debug_write("AW_VMM_FAIL reason=kernel_range_overflow\n");
        return None;
    };
    let Some(kernel_range) = PhysicalRange::new(kernel_image.physical_address, kernel_end) else {
        debug_write("AW_VMM_FAIL reason=kernel_range_shape\n");
        return None;
    };

    // The one allocator that owns physical frames from here on: the page tables
    // built below and every runtime mapping draw from it, so a frame spent on a
    // table is never handed back out (dossier section 7).
    // SAFETY: CPL0 single-core bootstrap, called once before any allocation.
    if !unsafe { frame_allocator::init(descriptors, kernel_range) } {
        debug_write("AW_VMM_FAIL reason=frame_allocator_init\n");
        return None;
    }
    debug_write("AW_FRAME_ALLOCATOR_OK\n");

    // The kernel-owned map marks every non-code page NX, but the NX bit is only
    // valid with EFER.NXE set. QEMU/OVMF leaves it on, so the switch worked there;
    // some firmware (VMware's EFI, for one) leaves it off, and then NX is a
    // reserved bit that faults the first access to any NX page the instant the new
    // CR3 loads - the kernel would triple-fault right after the switch. Enable it
    // here, before the map goes live, so the switch is safe whatever the firmware
    // left behind. The security baseline re-asserts it later; this is idempotent.
    const IA32_EFER_MSR: u32 = 0xc000_0080;
    const EFER_NXE: u64 = 1 << 11;
    // SAFETY: CPL0; EFER exists on every long-mode CPU, and enabling NXE before any
    // NX mapping is installed only makes the NX bits this kernel sets take effect.
    unsafe {
        let efer = local_apic::rdmsr(IA32_EFER_MSR);
        if efer & EFER_NXE == 0 {
            local_apic::wrmsr(IA32_EFER_MSR, efer | EFER_NXE);
        }
    }
    debug_write("AW_VMM_NXE_ON\n");
    if virtual_memory::supports_1gib_pages() {
        debug_write("AW_VMM_LEAF_MAX size=1g\n");
    } else {
        debug_write("AW_VMM_LEAF_MAX size=2m fallback=1\n");
    }

    // Build the exact firmware MMIO ranges that must survive the CR3 switch.
    // ECAM reserves 1 MiB of configuration space per bus by specification.
    let mut mmio_ranges = [(0_u64, 0_u64); MAX_HANDOFF_MMIO_RANGES];
    let mut mmio_count = 0_usize;

    if handoff.flags & HANDOFF_FLAG_FRAMEBUFFER_PRESENT != 0 {
        let framebuffer = handoff.framebuffer;
        let Some(end) = framebuffer
            .physical_address
            .checked_add(framebuffer.byte_len)
        else {
            debug_write("AW_VMM_FAIL reason=framebuffer_range_overflow\n");
            return None;
        };
        mmio_ranges[mmio_count] = (framebuffer.physical_address, end);
        mmio_count += 1;
    }

    if handoff.flags & HANDOFF_FLAG_PCIE_ECAM_PRESENT != 0 {
        for region in handoff
            .pcie_ecam
            .iter()
            .take(handoff.pcie_ecam_count as usize)
            .copied()
        {
            if mmio_count >= mmio_ranges.len() {
                debug_write("AW_VMM_FAIL reason=too_many_mmio_ranges\n");
                return None;
            }
            let bus_count = u64::from(region.end_bus) - u64::from(region.start_bus) + 1;
            let Some(byte_len) = bus_count.checked_mul(ECAM_BUS_BYTES) else {
                debug_write("AW_VMM_FAIL reason=ecam_size_overflow\n");
                return None;
            };
            let Some(end) = region.base_address.checked_add(byte_len) else {
                debug_write("AW_VMM_FAIL reason=ecam_range_overflow\n");
                return None;
            };
            mmio_ranges[mmio_count] = (region.base_address, end);
            mmio_count += 1;
        }
    }

    // xHCI BARs are commonly allocated above 4 GiB (QEMU uses 0xC000000000).
    // Discover the controller while firmware page tables are still active, then
    // carry a bounded RW+NX identity window into the kernel-owned page tables.
    if let Some(controller) = xhci::find(handoff) {
        if mmio_count >= mmio_ranges.len() {
            debug_write("AW_VMM_FAIL reason=too_many_mmio_ranges\n");
            return None;
        }
        let Some(end) = controller.bar0.checked_add(xhci::MMIO_WINDOW_BYTES) else {
            debug_write("AW_VMM_FAIL reason=xhci_range_overflow\n");
            return None;
        };
        mmio_ranges[mmio_count] = (controller.bar0, end);
        mmio_count += 1;
        debug_write("AW_VMM_XHCI_MMIO_RANGE base=");
        debug_write_hex_u64(controller.bar0);
        debug_write(" end=");
        debug_write_hex_u64(end);
        debug_write("\n");
    }

    // UEFI runtime code (Memory Attributes Table): mapped read-only and executable.
    let mut firmware_code = [(0_u64, 0_u64); aw_kernel_core::MAX_FIRMWARE_CODE_RANGES];
    let firmware_code_count = firmware_runtime::code_ranges(handoff, &mut firmware_code);

    // SAFETY: CPL0 single-core bootstrap after IDT/TSS install. Page-table
    // frames come from conventional RAM outside the kernel image. The map keeps
    // the low bootstrap window and all handed-off framebuffer/ECAM ranges
    // identity-mapped, so later device access remains valid after CR3 changes.
    let map = unsafe {
        virtual_memory::activate(
            frame_allocator::allocate,
            interrupts::double_fault_guard_page(),
            &mmio_ranges[..mmio_count],
            &firmware_code[..firmware_code_count],
        )
    };

    match map {
        Ok(map) => {
            debug_write("AW_VMM_CR3 prev=");
            debug_write_hex_u64(map.previous_cr3);
            debug_write(" new=");
            debug_write_hex_u64(map.cr3);
            debug_write("\n");
            debug_write("AW_VMM_IDENTITY_MAP_OK gib=");
            debug_write_u8(virtual_memory::IDENTITY_GIB as u8);
            debug_write(" tables=");
            debug_write_u64(map.table_count as u64);
            debug_write("\n");
            debug_write("AW_VMM_HANDOFF_MMIO_OK ranges=");
            debug_write_u64(map.extra_identity_range_count as u64);
            debug_write("\n");
            debug_write("AW_VMM_WX_LAYOUT text=");
            debug_write_hex_u64(map.layout.text.0);
            debug_write("..");
            debug_write_hex_u64(map.layout.text.1);
            debug_write(" data=");
            debug_write_hex_u64(map.layout.data.0);
            debug_write("..");
            debug_write_hex_u64(map.layout.data.1);
            debug_write(" guard=");
            debug_write_hex_u64(map.guard_page);
            debug_write("\n");
            debug_write("AW_VMM_FIRMWARE_CODE_RX ranges=");
            debug_write_u64(firmware_code_count as u64);
            debug_write("\n");
            firmware_runtime::set_kernel_map(map.previous_cr3, firmware_code_count > 0);
            facts::KERNEL_MAP_WX.store(true, core::sync::atomic::Ordering::Release);
            debug_write("AW_VMM_ACTIVE\n");
            Some(map)
        }
        Err(error) => {
            debug_write("AW_VMM_FAIL reason=");
            debug_write(error.name());
            debug_write("\n");
            None
        }
    }
}

/// Enable the CPU protection bits, then prove each one with a real fault.
///
/// Nothing here is claimed from a flag or a build success: every protection is
/// asserted only after the CPU has actually refused the corresponding access
/// with the expected `#PF` error code (dossier section 7 and DOD-03).
fn prove_memory_protections(map: &virtual_memory::ActiveMap) {
    debug_write("AW_SECURITY_BASELINE_BEGIN\n");
    // SAFETY: CPL0, single-core bootstrap, after the kernel owns its page
    // tables and with no user-accessible page mapped anywhere.
    let state = unsafe { security_baseline::enforce_baseline() };
    debug_write("AW_SECURITY_ENFORCED wp=");
    debug_write_u8(u8::from(state.cr0_write_protect));
    debug_write(" nx=");
    debug_write_u8(u8::from(state.efer_nx_enable));
    debug_write(" smep=");
    debug_write_u8(u8::from(state.cr4_smep));
    debug_write(" smap=");
    debug_write_u8(u8::from(state.cr4_smap));
    debug_write(" umip=");
    debug_write_u8(u8::from(state.cr4_umip));
    debug_write("\n");

    // SAFETY: CPL0, read-only.
    let baseline_ok = match unsafe { security_baseline::first_security_gap() } {
        None => {
            debug_write("AW_SECURITY_BASELINE_OK\n");
            true
        }
        Some(gap) => {
            debug_write("AW_SECURITY_BASELINE_GAP reason=");
            debug_write(gap.name());
            debug_write("\n");
            false
        }
    };

    debug_write("AW_MEMORY_PROTECTION_BEGIN\n");

    // Each probe targets a page whose permissions the map audit already
    // verified, so a missing fault means the CPU is not enforcing them.
    // SAFETY: every target is a kernel-owned page of the running image; the
    // probes recover through the armed exception path and never run foreign
    // code or corrupt live data.
    // The guard page is the first page of `.data`, so the NX probes target
    // pages that are definitely present: otherwise a not-present fault would
    // masquerade as an NX fault and prove nothing about NX.
    let writable_page = map.layout.data.1 - 4096;
    let proofs = [
        ProtectionProof {
            name: "nx-execute-rodata",
            outcome: unsafe { memory_protection::probe_execute_data(map.layout.rodata.0) },
        },
        ProtectionProof {
            name: "nx-execute-data",
            outcome: unsafe { memory_protection::probe_execute_data(writable_page) },
        },
        ProtectionProof {
            name: "wx-write-text",
            outcome: unsafe { memory_protection::probe_write_readonly(map.layout.text.0) },
        },
        ProtectionProof {
            name: "guard-page",
            outcome: unsafe { memory_protection::probe_touch_guard_page(map.guard_page) },
        },
    ];

    let all_passed = proofs.iter().all(|proof| proof.outcome.is_pass());
    for proof in proofs {
        match proof.outcome {
            ProofOutcome::Faulted(fault) => {
                debug_write("AW_MEMORY_PROTECTION_OK name=");
                debug_write(proof.name);
                debug_write(" error_code=");
                debug_write_hex_u64(fault.error_code);
                debug_write(" address=");
                debug_write_hex_u64(fault.address);
                debug_write("\n");
            }
            ProofOutcome::NoFault => {
                debug_write("AW_MEMORY_PROTECTION_FAIL name=");
                debug_write(proof.name);
                debug_write(" reason=no-fault\n");
            }
            ProofOutcome::WrongErrorCode(fault) => {
                debug_write("AW_MEMORY_PROTECTION_FAIL name=");
                debug_write(proof.name);
                debug_write(" reason=wrong-error-code error_code=");
                debug_write_hex_u64(fault.error_code);
                debug_write("\n");
            }
        }
    }

    if all_passed {
        debug_write("AW_MEMORY_PROTECTION_PROOF_OK\n");
        // Security health: CPU protections enforced and W^X proven by real faults.
        if baseline_ok {
            firmware_runtime::pass(aw_generation::RuntimeHealthCheck::Security);
        }
    }
}

/// First page above the 4 GiB low identity window. The runtime-mapping proof
/// maps here because the walk to it meets only interior tables - the window's
/// huge leaves stop at 4 GiB - so a fresh 4 KiB leaf can be added without
/// splitting anything.
const RUNTIME_MAP_TEST_VA: u64 = 0x1_0000_0000;

/// A non-zero pattern written through the freshly mapped page, distinct from the
/// zero a fresh frame holds, so a read-back that returns it proves the store
/// reached the mapped frame and not stale zeroes.
const RUNTIME_MAP_SENTINEL: u64 = 0xA11C_AB1E_5EED_F00D;

/// Prove the runtime map/unmap API edits the live page tables (dossier
/// section 7): map a frame at a fresh address, prove a store reaches it through
/// both the new address and the frame's identity address, prove translation
/// agrees, then unmap and prove the address now faults not-present.
fn prove_runtime_mapping() {
    debug_write("AW_VMM_RUNTIME_MAP_BEGIN\n");

    let Some(frame) = frame_allocator::allocate() else {
        debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=no_frame\n");
        return;
    };

    let flags =
        aw_x86_paging::PageTableFlags::WRITABLE.union(aw_x86_paging::PageTableFlags::NO_EXECUTE);
    // SAFETY: CPL0. The frame was just handed out by the sole frame owner, and
    // RUNTIME_MAP_TEST_VA is otherwise unused.
    if let Err(error) = unsafe { page_mapper::map_page(RUNTIME_MAP_TEST_VA, frame, flags) } {
        debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=map_");
        debug_write(error.name());
        debug_write("\n");
        return;
    }
    debug_write("AW_VMM_MAP_OK va=");
    debug_write_hex_u64(RUNTIME_MAP_TEST_VA);
    debug_write(" frame=");
    debug_write_hex_u64(frame);
    debug_write("\n");

    // Store through the new mapping; read it back through both the new address
    // and the frame's identity address (the frame is inside the identity
    // window). Agreement proves the mapping points where translation says.
    // SAFETY: the page is mapped writable above; the frame is identity-mapped.
    let (via_va, via_identity) = unsafe {
        core::ptr::write_volatile(RUNTIME_MAP_TEST_VA as *mut u64, RUNTIME_MAP_SENTINEL);
        (
            core::ptr::read_volatile(RUNTIME_MAP_TEST_VA as *const u64),
            core::ptr::read_volatile(frame as *const u64),
        )
    };
    if via_va != RUNTIME_MAP_SENTINEL || via_identity != RUNTIME_MAP_SENTINEL {
        debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=readback\n");
        return;
    }
    debug_write("AW_VMM_MAP_READBACK_OK\n");

    match page_mapper::translate(RUNTIME_MAP_TEST_VA) {
        Some((resolved, resolved_flags))
            if resolved == frame
                && resolved_flags.contains(aw_x86_paging::PageTableFlags::NO_EXECUTE)
                && !resolved_flags.contains(aw_x86_paging::PageTableFlags::USER_ACCESSIBLE) =>
        {
            debug_write("AW_VMM_MAP_TRANSLATE_OK\n");
        }
        _ => {
            debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=translate\n");
            return;
        }
    }

    // SAFETY: CPL0; the address was mapped by this function.
    match unsafe { page_mapper::unmap_page(RUNTIME_MAP_TEST_VA) } {
        Ok(returned) if returned == frame => debug_write("AW_VMM_UNMAP_OK\n"),
        _ => {
            debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=unmap\n");
            return;
        }
    }
    if page_mapper::translate(RUNTIME_MAP_TEST_VA).is_some() {
        debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=still_mapped\n");
        return;
    }

    // The address is unmapped, so touching it must take a not-present #PF. This
    // is the negative test: the mapping is gone from the CPU, not just the
    // tables, because the leaf was invalidated in the TLB.
    // SAFETY: RUNTIME_MAP_TEST_VA was just unmapped, so the probe faults and
    // recovers exactly like the guard-page probe.
    match unsafe { memory_protection::probe_touch_guard_page(RUNTIME_MAP_TEST_VA) } {
        ProofOutcome::Faulted(_) => debug_write("AW_VMM_UNMAP_FAULT_OK\n"),
        _ => {
            debug_write("AW_VMM_RUNTIME_MAP_FAIL reason=no_unmap_fault\n");
            return;
        }
    }

    debug_write("AW_VMM_RUNTIME_MAP_PROOF_OK\n");
}

/// Give the bootstrap processor its own GS-reachable per-CPU block before the
/// first interrupt is taken.
///
/// This must run before the timer and device delivery proofs: the ISRs count
/// into `gs:[0]`, so a block installed afterwards would leave the bootstrap
/// processor's per-CPU counters at zero even though it ran every handler. x2APIC
/// is enabled here only to read this CPU's APIC id; the timer proof enables it
/// again, which is idempotent (dossier section 8, roadmap P0 step 5).
fn install_bootstrap_per_cpu() {
    debug_write("AW_PERCPU_BSP_BEGIN\n");

    // SAFETY: CPL0 bootstrap with interrupts masked. Enabling x2APIC is
    // idempotent and reading the APIC id MSR has no side effects.
    let apic_id = unsafe {
        match apic_timer::prepare_x2apic() {
            Ok(_) => local_apic::rdmsr(local_apic::X2APIC_ID_MSR) as u32,
            Err(_) => u32::MAX,
        }
    };

    // SAFETY: CPL0, run once for the bootstrap processor's slot 0.
    if unsafe { percpu::install(0, apic_id) } {
        // Record the bootstrap processor's own #DF IST bounds in its per-CPU block
        // so its fault handler range-checks the same stack whether or not a block
        // is installed.
        if let Some(block) = percpu::by_index(0) {
            let (start, top) = interrupts::bootstrap_ist1_bounds();
            block.set_ist1_bounds(start, top);
        }
        debug_write("AW_PERCPU_BSP_OK cpu=0 apic_id=");
        debug_write_u64(u64::from(apic_id));
        debug_write("\n");
    } else {
        debug_write("AW_PERCPU_BSP_FAIL\n");
    }
}

/// Per-CPU timer interrupts an application processor must take on its own Local
/// APIC before the proof accepts that a per-CPU timer delivers on it. More than
/// one, so a single spurious entry cannot pass it.
const PER_CPU_AP_REQUIRED_TICKS: u64 = 4;

/// Bound on how long the bootstrap processor samples an AP's per-CPU timer
/// counter. A spin count, because this runs before any calibrated time source;
/// an AP that reaches [`PER_CPU_AP_REQUIRED_TICKS`] exits the wait at once, so
/// this only caps the failure path of a timer that never delivers.
const PER_CPU_AP_TIMER_SPIN_BUDGET: u32 = 500_000_000;

/// Prove each online CPU owns a distinct GS-reachable per-CPU block, and that
/// the bootstrap processor's interrupt counters are driven per CPU.
///
/// The bootstrap processor took real timer and device interrupts during their
/// delivery proofs, so its per-CPU counters must be non-zero: a zero here would
/// mean the ISRs counted only into the shared global counter and not into the
/// block of the CPU that ran them. Each application processor armed its own
/// Local APIC timer and idles under interrupts, so its per-CPU timer counter
/// must advance too - proving a per-CPU timer really delivers on that CPU, not
/// only on the bootstrap processor (dossier section 8, roadmap P0 step 5).
fn prove_per_cpu_state() {
    debug_write("AW_PERCPU_BEGIN\n");

    let Some(bsp) = percpu::by_index(0) else {
        debug_write("AW_PERCPU_FAIL reason=bsp_absent\n");
        return;
    };

    debug_write("AW_PERCPU_BSP cpu=");
    debug_write_u64(u64::from(bsp.cpu_index()));
    debug_write(" apic_id=");
    debug_write_u64(u64::from(bsp.apic_id()));
    debug_write(" timer_ticks=");
    debug_write_u64(bsp.timer_ticks());
    debug_write(" device_ticks=");
    debug_write_u64(bsp.device_ticks());
    debug_write("\n");

    if bsp.timer_ticks() == 0 {
        debug_write("AW_PERCPU_FAIL reason=bsp_no_timer_ticks\n");
        return;
    }
    if bsp.device_ticks() == 0 {
        debug_write("AW_PERCPU_FAIL reason=bsp_no_device_ticks\n");
        return;
    }

    let mut proven = 1; // the bootstrap processor
    let mut aps_with_timer = 0;
    let mut all_ok = true;
    for cpu in 1..interrupts::MAX_CPUS {
        let Some(summary) = smp::ap_summary(cpu) else {
            continue;
        };
        let Some(block) = percpu::by_index(cpu) else {
            debug_write("AW_PERCPU_FAIL reason=ap_block_absent cpu=");
            debug_write_u64(cpu as u64);
            debug_write("\n");
            all_ok = false;
            continue;
        };

        // The AP idles under its own Local APIC timer, so its per-CPU counter
        // advances on its own. Sample it until it reaches the bar or a bounded
        // budget runs out, so a CPU whose timer never delivers fails here rather
        // than hanging bring-up.
        let mut ap_timer_ticks = block.timer_ticks();
        let mut budget = PER_CPU_AP_TIMER_SPIN_BUDGET;
        while ap_timer_ticks < PER_CPU_AP_REQUIRED_TICKS && budget > 0 {
            core::hint::spin_loop();
            budget -= 1;
            ap_timer_ticks = block.timer_ticks();
        }

        debug_write("AW_PERCPU_AP cpu=");
        debug_write_u64(u64::from(block.cpu_index()));
        debug_write(" apic_id=");
        debug_write_u64(u64::from(block.apic_id()));
        debug_write(" timer_ticks=");
        debug_write_u64(ap_timer_ticks);
        debug_write("\n");

        if block.cpu_index() as usize != cpu || block.apic_id() != summary.reported_apic_id {
            debug_write("AW_PERCPU_FAIL reason=ap_identity_mismatch cpu=");
            debug_write_u64(cpu as u64);
            debug_write("\n");
            all_ok = false;
            continue;
        }
        if ap_timer_ticks < PER_CPU_AP_REQUIRED_TICKS {
            debug_write("AW_PERCPU_FAIL reason=ap_no_timer_ticks cpu=");
            debug_write_u64(cpu as u64);
            debug_write("\n");
            all_ok = false;
            continue;
        }
        proven += 1;
        aps_with_timer += 1;
    }

    if all_ok {
        debug_write("AW_PERCPU_PROOF_OK cpus=");
        debug_write_u64(proven as u64);
        debug_write("\n");
        if aps_with_timer > 0 {
            debug_write("AW_PERCPU_AP_TIMER_OK aps=");
            debug_write_u64(aps_with_timer as u64);
            debug_write("\n");
        }
    }
}

/// Number of timer interrupts the delivery proof requires before it accepts
/// that hardware interrupt delivery works. More than one, so a single spurious
/// or self-inflicted entry cannot pass it.
const APIC_TIMER_REQUIRED_TICKS: u64 = 8;

/// Run the Local APIC timer delivery proof and report it on the debug console.
///
/// Returns normally whether or not the proof passes: an absent or broken timer
/// must not stop the rest of bootstrap from reporting its own state, and the
/// markers make the failure explicit instead of silent.
fn prove_apic_timer_delivery() -> bool {
    debug_write("AW_APIC_TIMER_BEGIN\n");

    // SAFETY: runs once after IDT/TSS install, with interrupts still masked.
    // The legacy PIC is remapped and fully masked before the first `sti`, so an
    // unremapped IRQ0 cannot alias the #DF vector.
    if let Err(reason) = unsafe { apic_timer::arm_periodic_after_idt() } {
        debug_write("AW_APIC_TIMER_UNAVAILABLE reason=");
        debug_write(reason);
        debug_write("\n");
        return false;
    }
    // SAFETY: CPL0; the PIC is reprogrammed and fully masked before any `sti`.
    unsafe { legacy_pic::remap_and_mask_all() };
    debug_write("AW_APIC_TIMER_ARMED mode=periodic vector=");
    debug_write_u8(interrupt_vectors::APIC_TIMER_VECTOR);
    debug_write("\n");

    // SAFETY: CPL0, immediately after arming. Returns with interrupts disabled
    // and the timer masked, so the idle loop keeps its documented invariant.
    let proof = unsafe { apic_timer::run_delivery_proof(APIC_TIMER_REQUIRED_TICKS) };

    match proof {
        DeliveryProof::Passed {
            ticks_after_run,
            ticks_while_masked,
            ticks_after_unmask,
        } => {
            debug_write("AW_APIC_TIMER_FIRED\n");
            debug_write("AW_APIC_TIMER_MONOTONIC_OK ticks=");
            debug_write_u64(ticks_after_run);
            debug_write(" required=");
            debug_write_u64(APIC_TIMER_REQUIRED_TICKS);
            debug_write("\n");
            debug_write("AW_APIC_TIMER_MASKED_STOPPED ticks=");
            debug_write_u64(ticks_while_masked);
            debug_write("\n");
            debug_write("AW_APIC_TIMER_UNMASKED_RESUMED ticks=");
            debug_write_u64(ticks_after_unmask);
            debug_write("\n");
            debug_write("AW_APIC_TIMER_DELIVERY_PROOF_OK\n");
        }
        DeliveryProof::NotDelivered { ticks } => {
            debug_write("AW_APIC_TIMER_NOT_FIRED ticks=");
            debug_write_u64(ticks);
            debug_write("\n");
        }
        DeliveryProof::MaskIneffective { before, after } => {
            // The counter moved with the vector masked, so the increments are
            // not attributable to real interrupt delivery.
            debug_write("AW_APIC_TIMER_MASK_INEFFECTIVE before=");
            debug_write_u64(before);
            debug_write(" after=");
            debug_write_u64(after);
            debug_write("\n");
        }
        DeliveryProof::DidNotResume { ticks } => {
            debug_write("AW_APIC_TIMER_DID_NOT_RESUME ticks=");
            debug_write_u64(ticks);
            debug_write("\n");
        }
    }
    matches!(proof, DeliveryProof::Passed { .. })
}

/// Route a real device interrupt through an I/O APIC and prove it arrives.
///
/// Unlike the local APIC timer, nothing on this path is internal to the CPU:
/// the 8254 drives an interrupt pin, the I/O APIC translates that pin into the
/// vector its redirection entry names, and only then does the CPU see anything.
/// Which pin is not guessed - the MADT's interrupt source overrides decide it,
/// and on most platforms ISA IRQ 0 is not global system interrupt 0.
fn prove_device_interrupt_routing(handoff: &KernelHandoff) {
    debug_write("AW_IOAPIC_BEGIN\n");

    // SAFETY: the identity map is active and the RSDP address comes from the
    // handoff the loader already validated.
    let madt = match unsafe { acpi::find_madt(handoff.acpi_rsdp) } {
        Ok(madt) => madt,
        Err(error) => {
            debug_write("AW_IOAPIC_UNAVAILABLE reason=madt_");
            debug_write(error.name());
            debug_write("\n");
            return;
        }
    };

    debug_write("AW_MADT_OK local_apic=");
    debug_write_hex_u64(u64::from(madt.local_apic_address()));
    debug_write(" dual_8259=");
    debug_write_u8(u8::from(madt.dual_8259_present()));
    debug_write("\n");

    // SAFETY: CPL0 with interrupts disabled, after the IDT is installed and the
    // local APIC is running in x2APIC mode.
    let routed = match unsafe { device_irq::route_pit_through_ioapic(madt) } {
        Ok(routed) => routed,
        Err(reason) => {
            debug_write("AW_IOAPIC_UNAVAILABLE reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };

    let routing = routed.routing;
    debug_write("AW_IOAPIC_FOUND id=");
    debug_write_u8(routing.io_apic_id);
    debug_write(" madt_id=");
    debug_write_u8(routing.madt_id);
    debug_write(" base=");
    debug_write_hex_u64(routing.base);
    debug_write(" entries=");
    debug_write_u64(u64::from(routing.entry_count));
    debug_write("\n");
    debug_write("AW_IOAPIC_ROUTED isa_irq=");
    debug_write_u8(device_irq::PIT_ISA_IRQ);
    debug_write(" gsi=");
    debug_write_u64(u64::from(routing.global_system_interrupt));
    debug_write(" index=");
    debug_write_u64(u64::from(routing.redirection_index));
    debug_write(" vector=");
    debug_write_hex_u64(u64::from(routing.vector));
    debug_write("\n");

    // SAFETY: CPL0 on a route this function just programmed, with the legacy
    // PIC already remapped and fully masked by the APIC timer proof.
    match unsafe { device_irq::prove_routed_delivery(&routed) } {
        DeliveryProof::Passed {
            ticks_after_run,
            ticks_while_masked,
            ticks_after_unmask,
        } => {
            debug_write("AW_IOAPIC_IRQ_FIRED\n");
            debug_write("AW_IOAPIC_IRQ_MONOTONIC_OK ticks=");
            debug_write_u64(ticks_after_run);
            debug_write(" required=");
            debug_write_u64(device_irq::REQUIRED_TICKS);
            debug_write("\n");
            debug_write("AW_IOAPIC_MASKED_STOPPED ticks=");
            debug_write_u64(ticks_while_masked);
            debug_write("\n");
            debug_write("AW_IOAPIC_UNMASKED_RESUMED ticks=");
            debug_write_u64(ticks_after_unmask);
            debug_write("\n");
            debug_write("AW_IOAPIC_DELIVERY_PROOF_OK\n");
        }
        DeliveryProof::NotDelivered { ticks } => {
            debug_write("AW_IOAPIC_IRQ_NOT_FIRED ticks=");
            debug_write_u64(ticks);
            debug_write("\n");
        }
        DeliveryProof::MaskIneffective { before, after } => {
            debug_write("AW_IOAPIC_MASK_INEFFECTIVE before=");
            debug_write_u64(before);
            debug_write(" after=");
            debug_write_u64(after);
            debug_write("\n");
        }
        DeliveryProof::DidNotResume { ticks } => {
            debug_write("AW_IOAPIC_DID_NOT_RESUME ticks=");
            debug_write_u64(ticks);
            debug_write("\n");
        }
    }
}

fn pci_read_u32(address: PciAddress, register_offset: u8) -> Option<u32> {
    let config_address = address.mechanism1_address(register_offset)?;
    // SAFETY: PCI configuration mechanism #1 uses the architected CF8/CFC
    // dword I/O ports. This is retained as a compatibility fallback.
    unsafe {
        outl(PCI_CONFIG_ADDRESS_PORT, config_address);
        Some(inl(PCI_CONFIG_DATA_PORT))
    }
}

/// Start the application processors and prove each one really runs, on tables
/// of its own.
///
/// "Online" is not claimed from a counter the bootstrap processor increments.
/// Each AP reports the APIC ID it read from its *own* local APIC, plus the GDT,
/// TSS and IST1 addresses it actually loaded - values no other CPU could have
/// produced, and which must all differ from each other and from the bootstrap
/// processor's.
fn bring_up_secondary_processors(handoff: &KernelHandoff) {
    debug_write("AW_SMP_BEGIN\n");

    // SAFETY: the identity map is active and the RSDP comes from the validated
    // handoff.
    let madt = match unsafe { acpi::find_madt(handoff.acpi_rsdp) } {
        Ok(madt) => madt,
        Err(error) => {
            debug_write("AW_SMP_UNAVAILABLE reason=madt_");
            debug_write(error.name());
            debug_write("\n");
            return;
        }
    };

    // SAFETY: CPL0 on the bootstrap processor, after the IDT is installed, the
    // kernel owns its page tables and x2APIC is enabled.
    let result = match unsafe { smp::bring_up(handoff, madt) } {
        Ok(result) => result,
        Err(reason) => {
            debug_write("AW_SMP_UNAVAILABLE reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };

    debug_write("AW_SMP_CPUS described=");
    debug_write_u64(result.described as u64);
    debug_write(" bsp_apic_id=");
    debug_write_u64(u64::from(result.bootstrap_apic_id));
    debug_write("\n");

    let (bsp_gdt, bsp_tss) = interrupts::bootstrap_tables();
    let mut tables_are_private = result.online > 0;
    let mut seen: [(u64, u64, u32); interrupts::MAX_CPUS] = [(0, 0, 0); interrupts::MAX_CPUS];
    let mut seen_count = 0;

    for cpu in 1..interrupts::MAX_CPUS {
        let Some(summary) = smp::ap_summary(cpu) else {
            continue;
        };

        debug_write("AW_SMP_AP_ONLINE cpu=");
        debug_write_u64(summary.cpu as u64);
        debug_write(" apic_id=");
        debug_write_u64(u64::from(summary.reported_apic_id));
        debug_write(" requested=");
        debug_write_u64(u64::from(summary.requested_apic_id));
        debug_write(" tr=");
        debug_write_hex_u64(u64::from(summary.task_register));
        debug_write(" gdt=");
        debug_write_hex_u64(summary.gdt_base);
        debug_write(" tss=");
        debug_write_hex_u64(summary.tss_base);
        debug_write(" ist1=");
        debug_write_hex_u64(summary.ist1_top);
        debug_write("\n");

        // The AP that answered must be the one that was asked, and it must not
        // be sharing a descriptor table with anyone.
        if summary.reported_apic_id != summary.requested_apic_id
            || summary.gdt_base == bsp_gdt
            || summary.tss_base == bsp_tss
            || seen[..seen_count].iter().any(|&(gdt, tss, apic_id)| {
                gdt == summary.gdt_base
                    || tss == summary.tss_base
                    || apic_id == summary.reported_apic_id
            })
        {
            tables_are_private = false;
        }

        seen[seen_count] = (summary.gdt_base, summary.tss_base, summary.reported_apic_id);
        seen_count += 1;
    }

    debug_write("AW_SMP_ONLINE online=");
    debug_write_u64(result.online as u64);
    debug_write(" started=");
    debug_write_u64(result.started as u64);
    debug_write("\n");

    if result.started == 0 {
        debug_write("AW_SMP_NO_APPLICATION_PROCESSORS\n");
        return;
    }
    if result.online != result.started {
        debug_write("AW_SMP_AP_NOT_ONLINE\n");
        return;
    }
    if !tables_are_private {
        debug_write("AW_SMP_TABLES_SHARED\n");
        return;
    }

    debug_write("AW_SMP_PER_CPU_TABLES_OK cpus=");
    debug_write_u64(seen_count as u64);
    debug_write("\n");
    debug_write("AW_SMP_ALL_ONLINE\n");
}

/// Prove an MSI arrives: the device writes the interrupt into the local APIC
/// itself, with no pin and no I/O APIC anywhere in the path.
///
/// Only built with `msi-proof-device`, which also pulls in the driver for the
/// emulator test device this drives.
#[cfg(feature = "msi-proof-device")]
fn prove_msi_delivery(handoff: &KernelHandoff) {
    debug_write("AW_MSI_BEGIN\n");

    // SAFETY: CPL0 with interrupts disabled, after the IDT is installed and the
    // local APIC is running in x2APIC mode.
    let device = match unsafe { msi_proof::program_msi_device(handoff) } {
        Ok(device) => device,
        Err(reason) => {
            debug_write("AW_MSI_UNAVAILABLE reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };

    debug_write("AW_MSI_DEVICE_FOUND bus=");
    debug_write_u8(device.bus);
    debug_write(" device=");
    debug_write_u8(device.device);
    debug_write(" function=");
    debug_write_u8(device.function);
    debug_write("\n");
    debug_write("AW_MSI_PROGRAMMED vector=");
    debug_write_hex_u64(u64::from(device.vector));
    debug_write(" address=");
    debug_write_hex_u64(u64::from(device.message.address));
    debug_write(" data=");
    debug_write_hex_u64(u64::from(device.message.data));
    debug_write("\n");

    // SAFETY: CPL0 on a device this function just programmed.
    match unsafe { msi_proof::prove_msi_delivery(&device) } {
        DeliveryProof::Passed {
            ticks_after_run,
            ticks_while_masked,
            ticks_after_unmask,
        } => {
            debug_write("AW_MSI_FIRED\n");
            debug_write("AW_MSI_MONOTONIC_OK ticks=");
            debug_write_u64(ticks_after_run);
            debug_write(" required=");
            debug_write_u64(msi_proof::REQUIRED_TICKS);
            debug_write("\n");
            debug_write("AW_MSI_MASKED_STOPPED ticks=");
            debug_write_u64(ticks_while_masked);
            debug_write("\n");
            debug_write("AW_MSI_UNMASKED_RESUMED ticks=");
            debug_write_u64(ticks_after_unmask);
            debug_write("\n");
            debug_write("AW_MSI_DELIVERY_PROOF_OK\n");
        }
        DeliveryProof::NotDelivered { ticks } => {
            debug_write("AW_MSI_NOT_FIRED ticks=");
            debug_write_u64(ticks);
            debug_write("\n");
        }
        DeliveryProof::MaskIneffective { before, after } => {
            debug_write("AW_MSI_MASK_INEFFECTIVE before=");
            debug_write_u64(before);
            debug_write(" after=");
            debug_write_u64(after);
            debug_write("\n");
        }
        DeliveryProof::DidNotResume { ticks } => {
            debug_write("AW_MSI_DID_NOT_RESUME ticks=");
            debug_write_u64(ticks);
            debug_write("\n");
        }
    }
}

fn classify_pci_device(identity: PciDeviceIdentity, found: &mut [bool; 4]) {
    if identity.class.is_nvme() {
        found[0] = true;
    }
    if identity.class.is_ahci() {
        found[1] = true;
    }
    if identity.class.is_xhci() {
        found[2] = true;
    }
    if identity.class.is_hda() {
        found[3] = true;
    }
}

fn emit_pci_classes(found: [bool; 4]) {
    if found[0] {
        debug_write("AW_PCI_NVME_FOUND\n");
    }
    if found[1] {
        debug_write("AW_PCI_AHCI_FOUND\n");
    }
    if found[2] {
        debug_write("AW_PCI_XHCI_FOUND\n");
    }
    if found[3] {
        debug_write("AW_PCI_HDA_FOUND\n");
    }
}

fn scan_pcie_ecam(handoff: &KernelHandoff) -> bool {
    if handoff.flags & HANDOFF_FLAG_PCIE_ECAM_PRESENT == 0 || handoff.pcie_ecam_count == 0 {
        return false;
    }

    debug_write("AW_PCIE_ECAM_SCAN_BEGIN\n");
    let mut any_device = false;
    let mut found = [false; 4];

    for region in handoff
        .pcie_ecam
        .iter()
        .take(handoff.pcie_ecam_count as usize)
        .copied()
    {
        for bus in region.start_bus..=region.end_bus {
            for device in 0_u8..32 {
                let vendor_device0 =
                    pci_config::read_u32(region, bus, device, 0, 0x00).unwrap_or(u32::MAX);
                if vendor_device0 as u16 == 0xffff {
                    continue;
                }

                let header_register =
                    pci_config::read_u32(region, bus, device, 0, 0x0c).unwrap_or(0);
                let header_type = ((header_register >> 16) & 0xff) as u8;
                let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };

                for function in 0_u8..function_count {
                    let vendor_device = pci_config::read_u32(region, bus, device, function, 0x00)
                        .unwrap_or(u32::MAX);
                    if vendor_device as u16 == 0xffff {
                        continue;
                    }

                    let class_revision =
                        pci_config::read_u32(region, bus, device, function, 0x08).unwrap_or(0);
                    let subsystem = pci_config::read_u32(region, bus, device, function, 0x2c);
                    let identity = PciDeviceIdentity::from_config_registers(
                        vendor_device,
                        class_revision,
                        subsystem,
                    );
                    any_device = true;
                    classify_pci_device(identity, &mut found);
                }
            }
        }
    }

    if any_device {
        debug_write("AW_PCIE_ECAM_SCAN_OK\n");
        emit_pci_classes(found);
    } else {
        debug_write("AW_PCIE_ECAM_SCAN_EMPTY\n");
    }
    any_device
}

fn scan_pci_mechanism1() {
    debug_write("AW_PCI_SCAN_BEGIN mechanism=cf8_cfc\n");

    let mut any_device = false;
    let mut found = [false; 4];

    for bus in 0_u16..=255 {
        for device in 0_u8..32 {
            let function0 = match PciAddress::new(0, bus as u8, device, 0) {
                Some(address) => address,
                None => continue,
            };
            let vendor_device0 = pci_read_u32(function0, 0x00).unwrap_or(u32::MAX);
            if vendor_device0 as u16 == 0xffff {
                continue;
            }

            let header_register = pci_read_u32(function0, 0x0c).unwrap_or(0);
            let header_type = ((header_register >> 16) & 0xff) as u8;
            let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };

            for function in 0_u8..function_count {
                let address = match PciAddress::new(0, bus as u8, device, function) {
                    Some(value) => value,
                    None => continue,
                };
                let vendor_device = pci_read_u32(address, 0x00).unwrap_or(u32::MAX);
                if vendor_device as u16 == 0xffff {
                    continue;
                }

                let class_revision = pci_read_u32(address, 0x08).unwrap_or(0);
                let subsystem = pci_read_u32(address, 0x2c);
                let identity = PciDeviceIdentity::from_config_registers(
                    vendor_device,
                    class_revision,
                    subsystem,
                );
                any_device = true;
                classify_pci_device(identity, &mut found);
            }
        }
    }

    if !any_device {
        debug_write("AW_PCI_SCAN_FAIL reason=no_devices\n");
        return;
    }

    debug_write("AW_PCI_SCAN_OK\n");
    emit_pci_classes(found);
}

fn scan_pci(handoff: &KernelHandoff) {
    if scan_pcie_ecam(handoff) {
        debug_write("AW_PCI_TRANSPORT_OK transport=ecam\n");
    } else {
        debug_write("AW_PCI_TRANSPORT_FALLBACK transport=cf8_cfc\n");
        scan_pci_mechanism1();
    }
}

fn paint_boot_marker(handoff: &KernelHandoff) -> bool {
    if handoff.flags & HANDOFF_FLAG_FRAMEBUFFER_PRESENT == 0 {
        return false;
    }

    let framebuffer = handoff.framebuffer;
    if !matches!(
        framebuffer.pixel_format,
        HandoffPixelFormat::Rgb | HandoffPixelFormat::Bgr
    ) {
        return false;
    }

    let required_bytes = u64::from(framebuffer.stride_pixels)
        .saturating_mul(u64::from(framebuffer.height))
        .saturating_mul(4);
    if required_bytes > framebuffer.byte_len || framebuffer.physical_address == 0 {
        return false;
    }

    let rows = framebuffer.height.min(32);
    let width = framebuffer.width;
    let stride = u64::from(framebuffer.stride_pixels);
    let base = framebuffer.physical_address as *mut u32;

    for row in 0..rows {
        for column in 0..width {
            let pixel_index = u64::from(row)
                .saturating_mul(stride)
                .saturating_add(u64::from(column));
            if pixel_index >= framebuffer.byte_len / 4 {
                return false;
            }

            // SAFETY: The handoff was validated, bounds are checked above, and
            // this range is the UEFI-provided linear framebuffer.
            unsafe {
                core::ptr::write_volatile(base.add(pixel_index as usize), 0x00ff_00ff);
            }
        }
    }

    true
}

/// Native kernel entry point invoked by the UEFI loader after
/// `ExitBootServices`.
///
/// # Safety
///
/// The loader must pass a `handoff_ptr` that is either null or points at a
/// live, correctly initialized [`KernelHandoff`] that outlives this call. The
/// pointer is validated before any field other than nullness is trusted.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._start")]
pub unsafe extern "sysv64" fn _start(handoff_ptr: *const KernelHandoff) -> ! {
    // SAFETY: This is the first native kernel instruction path. No IDT exists
    // yet, so mask interrupts before touching any other CPU state.
    unsafe { asm!("cli", options(nomem, nostack, preserves_flags)) };

    if handoff_ptr.is_null() {
        debug_write("AW_NATIVE_KERNEL_FAIL reason=null_handoff\n");
        halt_forever();
    }

    // SAFETY: The UEFI loader constructs a KernelHandoff on its live stack and
    // transfers control without returning. The pointer remains valid here.
    let handoff = unsafe { &*handoff_ptr };
    if let Err(_error) = handoff.validate() {
        debug_write("AW_NATIVE_KERNEL_FAIL reason=invalid_handoff\n");
        halt_forever();
    }

    // SAFETY: _start masks interrupts before reaching this point. The tables
    // are installed once during single-core bootstrap.
    debug_write("AW_NATIVE_GDT_IDT_BEGIN\n");
    // `install()` emits AW_NATIVE_GDT_IDT_INSTALLED once the tables are live.
    unsafe { interrupts::install() };
    debug_write("AW_NATIVE_KERNEL_ENTRY_OK\n");

    // The three configurations below are mutually exclusive and each diverges,
    // so exactly one of them is the terminal path of `_start` in any given
    // build. Keeping the normal-boot continuation inside its own branch avoids
    // dead trailing code in the dedicated smoke-test images.
    #[cfg(feature = "double-fault-smoke-test")]
    {
        debug_write("AW_DOUBLE_FAULT_SMOKE_REQUEST\n");
        // SAFETY: dedicated smoke-test build; this intentionally causes a
        // delivery-time #GP failure so the CPU must enter #DF on IST1.
        unsafe { interrupts::trigger_double_fault_smoke() }
    }

    #[cfg(all(
        feature = "exception-smoke-test",
        not(feature = "double-fault-smoke-test")
    ))]
    {
        debug_write("AW_EXCEPTION_SMOKE_TRIGGER vector=6\n");
        // SAFETY: dedicated smoke-test build; `ud2` deterministically raises
        // #UD (vector 6) so the invalid-opcode handler can be observed.
        unsafe { asm!("ud2", options(noreturn)) }
    }

    #[cfg(not(any(feature = "exception-smoke-test", feature = "double-fault-smoke-test")))]
    {
        // Bring up the serial console first: it is the real-hardware diagnostic
        // channel the dossier asks for early in boot (section 4).
        serial::prove();
        facts::record_cpu_brand();

        // Bring up the framebuffer text console next, before anything that could
        // hang, so a physical machine shows the kernel took over the instant it
        // does - the debug port and COM1 that carry every marker do not exist on a
        // laptop, and ConOut is gone after ExitBootServices. Inert if the firmware
        // handed off no directly writable framebuffer. Prove it once it is up.
        framebuffer::init(handoff);
        framebuffer::write_line("omni-os");
        framebuffer::write_line("Kernel running (post-firmware). Bringing up the system...");
        framebuffer::prove();

        // Take over paging from the firmware before anything else in the
        // normal boot path, so the remaining bring-up runs on kernel-owned,
        // W^X page tables. Identity-mapped, so a failure here is non-fatal:
        // the firmware tables stay active and boot continues, with the
        // protections explicitly reported as unproven.
        let memory_ready = match activate_virtual_memory(handoff) {
            Some(map) => {
                prove_memory_protections(&map);
                prove_runtime_mapping();
                heap::prove();
                ring3::prove();
                scheduler::prove();
                true
            }
            None => {
                debug_write("AW_MEMORY_PROTECTION_SKIPPED reason=no-kernel-page-tables\n");
                false
            }
        };

        install_bootstrap_per_cpu();
        let timer_ready = prove_apic_timer_delivery();
        // The timer gate is installed and x2APIC is live; prove threads that never
        // yield are still switched by that timer. Leaves the timer masked and
        // interrupts disabled again, as the device-routing proof below expects.
        // SAFETY: CPL0 on the bootstrap processor, right after the timer proof.
        if timer_ready {
            unsafe { scheduler::prove_preemptive() };
        } else {
            debug_write("AW_PREEMPT_SKIPPED reason=timer-not-proven\n");
        }
        // With per-CPU GS and the timer both live, prove a CPL3 user thread that
        // never makes a syscall is preempted by the timer. Runs before SMP so the
        // single-CPU switch state is never touched by an application processor.
        // SAFETY: CPL0 on the bootstrap processor; per-CPU block installed.
        if timer_ready && memory_ready {
            unsafe { ring3::prove_ring3_preemption() };
        } else {
            debug_write("AW_RING3_PREEMPT_SKIPPED reason=timer-or-memory-not-ready\n");
        }
        prove_device_interrupt_routing(handoff);
        // Bring up the PS/2 keyboard and prove real IRQ1 delivery + decode, so the
        // machine can be driven by keyboard - the input half of an accessible boot.
        // Runs after the device-IRQ routing proof (I/O APIC reachable) and before
        // SMP, while only IRQ1 is unmasked, so delivery is unambiguous.
        // SAFETY: CPL0 on the bootstrap processor; IDT installed, x2APIC enabled.
        unsafe { ps2_keyboard::prove(handoff) };
        // Prove the transport-independent USB HID report -> menu-key contract.
        // This is deliberately not a claim that an xHCI device was driven yet.
        usb_hid_keyboard::prove_decode_path();
        // SMP reads x2APIC MSRs and shares the kernel-owned page tables.
        if timer_ready && memory_ready {
            bring_up_secondary_processors(handoff);
        } else {
            debug_write("AW_SMP_UNAVAILABLE reason=timer-or-memory-not-ready\n");
        }
        prove_per_cpu_state();
        clock::prove();
        // Read the wall-clock date/time from the CMOS RTC (read-only, safe on any
        // machine, so it runs on the normal boot path alongside the TSC clock).
        rtc::prove();
        // Speak the installer's first screen through the native screen reader:
        // build its accessible tree, validate the invariants, and emit the exact
        // utterance for each control. Device-free nonvisual delivery evidence.
        screen_reader::prove();
        // Render a spoken control to braille cells for a refreshable display:
        // semantic node -> utterance -> Grade 1 braille. Also device-free.
        braille::prove();
        debug_write("AW_VIRTIO_BLK_BEGIN\n");
        match virtio_blk::init() {
            Some(device) => {
                virtio_blk::prove(&device);
                fat16::prove(&device);
                // Load a userland ELF off that same filesystem and run it at CPL3.
                // SAFETY: CPL0; paging, heap and the first Ring 3 proof are up, and
                // the device was just brought up.
                if memory_ready {
                    unsafe { ring3::prove_user_loader(&device) };
                } else {
                    debug_write("AW_USER_LOADER_SKIPPED reason=no-kernel-page-tables\n");
                }
                // Then load two userland programs and preemptively schedule both.
                // SAFETY: same preconditions; runs before any AP is online.
                if memory_ready && timer_ready {
                    unsafe { ring3::prove_user_init(&device) };
                    // Then two programs that talk through a kernel channel reached
                    // only by handles, and the refusals of the handle model.
                    // SAFETY: same preconditions as the init proof.
                    unsafe { ring3::prove_user_ipc(&device) };
                } else {
                    debug_write("AW_USER_INIT_SKIPPED reason=timer-or-memory-not-ready\n");
                }
            }
            None => debug_write("AW_VIRTIO_BLK_UNAVAILABLE reason=no_device\n"),
        }
        debug_write("AW_VIRTIO_NET_BEGIN\n");
        virtio_net::prove();
        // A real SATA controller (AHCI), the kind VMware and physical PCs use.
        // The normal path only reads (safe on any disk, including a real boot
        // disk); the write proof is a dedicated test build against a scratch disk.
        // Partition a blank scratch disk first: write a GPT (the installer's
        // partitioning half), then the ordinary AHCI read and GPT read below run
        // against the table we just wrote and validate it. Scratch disk only,
        // gated out of the normal boot path; the 8 MiB scratch disk is 16384
        // sectors.
        #[cfg(feature = "gpt-write-smoke-test")]
        if let Some(port) = ahci::init() {
            gpt::prove_write(&port, 16384);
        } else {
            debug_write("AW_GPTWRITE_FAIL reason=no_ahci_port\n");
        }
        // Format a blank scratch disk as FAT16, then create a file in it and read
        // it back (the installer's format step). Runs before the read proves so the
        // freshly written boot sector is what they see. Scratch disk only, gated
        // out of the normal boot path; the 8 MiB scratch disk is 16384 sectors.
        #[cfg(feature = "fat-format-smoke-test")]
        if let Some(port) = ahci::init() {
            fat16::prove_format(&port, 0, 16384);
        } else {
            debug_write("AW_MKFS_FAIL reason=no_ahci_port\n");
        }
        // Capstone: build a complete installable disk from blank - GPT + FAT16 ESP +
        // a file - and read it back through the whole stack. Runs before the read
        // proves so they see the disk it built. Scratch disk only, gated out of the
        // normal boot path; the 8 MiB scratch disk is 16384 sectors.
        #[cfg(feature = "disk-build-smoke-test")]
        if let Some(port) = ahci::init() {
            installer::prove(&port, 16384);
        } else {
            debug_write("AW_DISKBUILD_FAIL reason=no_ahci_port\n");
        }
        #[cfg(not(feature = "ahci-write-smoke-test"))]
        ahci::prove();
        #[cfg(feature = "ahci-write-smoke-test")]
        ahci::prove_write();
        // Read the disk's own GPT partition table (read-only; reports unavailable
        // on a non-GPT disk, e.g. the write-smoke scratch disk).
        gpt::prove();
        // Bring up an NVMe controller and read its IDENTIFY data (read-only, safe
        // on any machine; reports unavailable when no controller is present).
        nvme::prove();
        // Bring up the HDA audio controller and read the codec's identity over
        // CORB/RIRB (read-only, safe on any machine; the foundation for spoken
        // screen-reader output). Reports unavailable when no controller is present.
        hda::prove();
        // Prove spoken output: play one real speech clip (the menu's own voice)
        // through the HDA codec and require the audio DMA to advance. Reports
        // unavailable (not a failure) on a machine with no HDA controller.
        hda::prove_speech(boot_menu::title_clip());
        // Create a file on a FAT16 scratch disk and read it back (installer
        // foundation). Scratch disk only: it modifies the filesystem, so it is
        // gated out of the normal boot path and never touches a real disk.
        #[cfg(feature = "fat-write-smoke-test")]
        if let Some(port) = ahci::init() {
            fat16::prove_fat_write(&port, 0);
        } else {
            debug_write("AW_FATWRITE_FAIL reason=no_ahci_port\n");
        }
        #[cfg(feature = "msi-proof-device")]
        prove_msi_delivery(handoff);

        if !validate_memory_map(handoff) {
            halt_forever();
        }
        if !probe_bootstrap_page_allocator(handoff) {
            halt_forever();
        }
        validate_cpu_baseline();
        scan_pci(handoff);
        // Identify the exact xHCI PCI function and BAR without taking ownership.
        xhci::prove_pci_discovery(handoff);
        // Read xHCI capability registers through the kernel-owned identity map.
        // Read-only: no reset, run/stop, doorbell or DMA programming yet.
        xhci::prove_mmio_capabilities(handoff, memory_ready);
        #[cfg(feature = "xhci-smoke-test")]
        xhci::prove_controller_smoke(handoff, memory_ready);

        if paint_boot_marker(handoff) {
            debug_write("AW_NATIVE_FRAMEBUFFER_WRITE_OK\n");
        } else {
            debug_write("AW_NATIVE_FRAMEBUFFER_WRITE_SKIP\n");
        }

        // Prove the accessible boot menu's navigation and selection logic (the
        // keyboard's real IRQ path is proved separately above).
        // Read the ACPI power controls (FADT + \_S5) the menu's Reboot and Power off use.
        // SAFETY: identity map active; the RSDP comes from the validated handoff.
        unsafe { power::init(handoff.acpi_rsdp) };

        boot_menu::prove();

        // Runtime health of this boot; during a trial attempt it is recorded for the loader,
        // which decides the promotion to known-good on the next boot.
        firmware_runtime::record(handoff);

        // The graphical administration session: every panel navigated and voiced, one utterance
        // rendered to real speech by the native synthesizer.
        admin::prove();

        debug_write("AW_NATIVE_KERNEL_IDLE\n");

        // Test-only: prove ACPI S5 power off (QEMU must exit on its own) or a reset
        // (a second boot must follow). Never enabled in a shipping image.
        #[cfg(feature = "acpi-poweroff-test")]
        // SAFETY: CPL0; the last action of this boot.
        unsafe {
            power::prove_power_off()
        }
        #[cfg(feature = "acpi-reset-test")]
        // SAFETY: CPL0; resets on the first boot, reports on the second.
        unsafe {
            power::prove_reset()
        };

        // Hand off to the interactive accessible menu, driven by the real
        // keyboard. Under headless boot no key ever arrives, so it parks under
        // `hlt`; the idle marker above has already satisfied the boot proof, so
        // the harness passes and then times out and stops the machine. On real
        // hardware this is where the user takes over.
        // SAFETY: CPL0 on the bootstrap processor; the keyboard was routed and
        // proved by `ps2_keyboard::prove`, so arming it for input is sound.
        unsafe { admin::run() }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    debug_write("AW_NATIVE_KERNEL_PANIC\n");
    halt_forever();
}
