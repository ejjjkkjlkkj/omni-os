#![no_main]
#![no_std]

extern crate alloc;

use aw_acpi::{McfgError, RsdpError, RsdpInfo, SdtError};
use aw_kernel_core::{
    AwknImageHeader, FramebufferHandoff, HandoffPixelFormat, KernelHandoff, KernelImageHandoff,
    MAX_PCIE_ECAM_REGIONS, MemoryDescriptorHandoff, MemoryMapHandoff, PciEcamHandoff,
};
use uefi::boot::{self, AllocateType};
use uefi::mem::memory_map::{MemoryMap, MemoryType};
use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat as UefiPixelFormat};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::table::cfg::ConfigTableEntry;
use uefi::{Status, system};

mod ac97;
mod audio;
mod hda;
mod hii_ifr;
mod measured;
mod net;
mod recovery;
mod screen_reader;
mod serial;
mod setup;
mod sound;
mod synth;
mod usb;
mod usb_audio;
mod virtio_snd;
mod word_bank;

/// Emit an accessibility marker to both the 0xE9 debug console (via the `uefi`
/// logger, which the QEMU proof suite asserts on) and the real COM1 line (which
/// VMware and physical hardware capture instead). One call site, two channels, so
/// the firmware-stage screen reader leaves the same machine-checkable trace on a
/// machine with no 0xE9 port as it does under QEMU.
macro_rules! aw_mark {
    ($($arg:tt)*) => {{
        log::info!($($arg)*);
        $crate::serial::mirror(format_args!($($arg)*));
    }};
}
pub(crate) use aw_mark;

const UEFI_PAGE_SIZE: usize = 4096;
const MAX_ACPI_SDT_LEN: usize = 1024 * 1024;
const NORMALIZED_MEMORY_MAP_PAGES: usize = 16;

#[derive(Clone, Copy, Debug)]
struct LoadedKernel {
    /// Absolute address of `_start` inside the loaded image.
    entry_address: usize,
    /// Physical base the image was placed at, i.e. its `AWKN` load base.
    image_base: usize,
    image_size: usize,
    allocation_size: usize,
}

#[derive(Clone, Copy, Debug)]
struct EcamDiscovery {
    regions: [PciEcamHandoff; MAX_PCIE_ECAM_REGIONS],
    count: u32,
}

impl EcamDiscovery {
    const NONE: Self = Self {
        regions: [PciEcamHandoff::NONE; MAX_PCIE_ECAM_REGIONS],
        count: 0,
    };
}

fn validate_firmware_rsdp(address: usize, table_revision: u8) -> Result<RsdpInfo, RsdpError> {
    let probe_len = if table_revision >= 2 {
        aw_acpi::RSDP_V2_MIN_LEN
    } else {
        aw_acpi::RSDP_V1_LEN
    };

    // SAFETY: `address` comes directly from the UEFI ACPI configuration-table
    // entry selected by its ACPI GUID. UEFI guarantees that this entry points
    // to an RSDP structure. We only borrow the minimum structure size here and
    // perform this validation before ExitBootServices.
    let probe = unsafe { core::slice::from_raw_parts(address as *const u8, probe_len) };
    let declared_length = aw_acpi::declared_length(probe)?;

    if declared_length <= probe.len() {
        return aw_acpi::validate_rsdp(&probe[..declared_length]);
    }

    // SAFETY: The validated RSDP prefix declares its own total length. The
    // parser caps that value at `RSDP_MAX_LEN`; UEFI's ACPI configuration-table
    // contract identifies the pointer as the complete RSDP. This expanded
    // borrow is also consumed before ExitBootServices and is not retained.
    let full = unsafe { core::slice::from_raw_parts(address as *const u8, declared_length) };
    aw_acpi::validate_rsdp(full)
}

fn borrow_valid_sdt(address: u64) -> Result<&'static [u8], SdtError> {
    if address == 0 || address > usize::MAX as u64 {
        return Err(SdtError::InvalidLength);
    }

    // SAFETY: The caller obtains SDT addresses from a validated ACPI root table.
    // We first read only the fixed 36-byte header and cap the declared length
    // before expanding the slice. All use occurs before ExitBootServices.
    let header = unsafe {
        core::slice::from_raw_parts(address as usize as *const u8, aw_acpi::SDT_HEADER_LEN)
    };
    let declared_length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if !(aw_acpi::SDT_HEADER_LEN..=MAX_ACPI_SDT_LEN).contains(&declared_length) {
        return Err(SdtError::InvalidLength);
    }

    // SAFETY: `declared_length` is bounded above. The pointer came from ACPI
    // firmware data reachable through a validated root table.
    let full =
        unsafe { core::slice::from_raw_parts(address as usize as *const u8, declared_length) };
    aw_acpi::validate_sdt(full)?;
    Ok(full)
}

fn discover_pcie_ecam(rsdp: &RsdpInfo) -> EcamDiscovery {
    let (root_address, entry_size, expected_signature) = match rsdp.xsdt_address {
        Some(xsdt) if xsdt != 0 => (xsdt, 8_usize, *b"XSDT"),
        _ if rsdp.rsdt_address != 0 => (u64::from(rsdp.rsdt_address), 4_usize, *b"RSDT"),
        _ => {
            log::warn!("AW_PCIE_ECAM_UNAVAILABLE reason=no_acpi_root");
            return EcamDiscovery::NONE;
        }
    };

    let root = match borrow_valid_sdt(root_address) {
        Ok(root) => root,
        Err(error) => {
            log::warn!(
                "AW_PCIE_ECAM_UNAVAILABLE reason=root_invalid error={:?}",
                error
            );
            return EcamDiscovery::NONE;
        }
    };

    if root[..4] != expected_signature {
        log::warn!(
            "AW_PCIE_ECAM_UNAVAILABLE reason=root_signature actual={:?}",
            &root[..4]
        );
        return EcamDiscovery::NONE;
    }

    let payload = &root[aw_acpi::SDT_HEADER_LEN..];
    if !payload.len().is_multiple_of(entry_size) {
        log::warn!("AW_PCIE_ECAM_UNAVAILABLE reason=root_entry_alignment");
        return EcamDiscovery::NONE;
    }

    let mut discovery = EcamDiscovery::NONE;
    let mut offset = 0_usize;
    while offset < payload.len() {
        let table_address = if entry_size == 8 {
            u64::from_le_bytes([
                payload[offset],
                payload[offset + 1],
                payload[offset + 2],
                payload[offset + 3],
                payload[offset + 4],
                payload[offset + 5],
                payload[offset + 6],
                payload[offset + 7],
            ])
        } else {
            u64::from(u32::from_le_bytes([
                payload[offset],
                payload[offset + 1],
                payload[offset + 2],
                payload[offset + 3],
            ]))
        };
        offset += entry_size;

        let table = match borrow_valid_sdt(table_address) {
            Ok(table) => table,
            Err(error) => {
                log::warn!(
                    "AW_ACPI_CHILD_SKIP address=0x{:x} error={:?}",
                    table_address,
                    error
                );
                continue;
            }
        };
        if table[..4] != *b"MCFG" {
            continue;
        }

        let mcfg = match aw_acpi::validate_mcfg(table) {
            Ok(mcfg) => mcfg,
            Err(error) => {
                match error {
                    McfgError::InvalidSdt(inner) => {
                        log::warn!("AW_PCIE_ECAM_UNAVAILABLE reason=mcfg_sdt error={:?}", inner)
                    }
                    other => log::warn!(
                        "AW_PCIE_ECAM_UNAVAILABLE reason=mcfg_invalid error={:?}",
                        other
                    ),
                }
                continue;
            }
        };

        for allocation in mcfg.allocations() {
            if discovery.count as usize >= MAX_PCIE_ECAM_REGIONS {
                log::warn!(
                    "AW_PCIE_ECAM_TRUNCATED max_regions={}",
                    MAX_PCIE_ECAM_REGIONS
                );
                return discovery;
            }

            let region = PciEcamHandoff {
                base_address: allocation.base_address,
                segment_group: allocation.segment_group,
                start_bus: allocation.start_bus,
                end_bus: allocation.end_bus,
                reserved: 0,
            };
            if !region.is_valid() {
                log::warn!(
                    "AW_PCIE_ECAM_REGION_SKIP base=0x{:x} segment={} buses={}-{}",
                    region.base_address,
                    region.segment_group,
                    region.start_bus,
                    region.end_bus
                );
                continue;
            }

            let index = discovery.count as usize;
            discovery.regions[index] = region;
            discovery.count += 1;
            log::info!(
                "AW_PCIE_ECAM_REGION_OK index={} base=0x{:x} segment={} buses={}-{}",
                index,
                region.base_address,
                region.segment_group,
                region.start_bus,
                region.end_bus
            );
        }
    }

    if discovery.count == 0 {
        log::warn!("AW_PCIE_ECAM_UNAVAILABLE reason=no_mcfg_allocation");
    } else {
        log::info!("AW_PCIE_ECAM_DISCOVERY_OK regions={}", discovery.count);
    }
    discovery
}

fn load_native_kernel() -> Result<LoadedKernel, Status> {
    let mut file_system = match boot::get_image_file_system(boot::image_handle()) {
        Ok(file_system) => {
            log::info!("AW_KERNEL_FS_OK source=image_handle");
            file_system
        }
        Err(error) => {
            log::warn!("AW_KERNEL_FS_FALLBACK image_status={:?}", error.status());
            let handle = boot::get_handle_for_protocol::<SimpleFileSystem>()
                .map_err(|error| error.status())?;
            let file_system = boot::open_protocol_exclusive::<SimpleFileSystem>(handle)
                .map_err(|error| error.status())?;
            log::info!("AW_KERNEL_FS_OK source=protocol_scan");
            file_system
        }
    };

    let mut root = file_system.open_volume().map_err(|error| {
        log::error!("AW_KERNEL_VOLUME_FAIL status={:?}", error.status());
        error.status()
    })?;

    loop {
        match root.read_entry_boxed() {
            Ok(Some(entry)) => log::info!(
                "AW_ESP_ENTRY name={:?} size={} directory={}",
                entry.file_name(),
                entry.file_size(),
                entry.is_directory()
            ),
            Ok(None) => break,
            Err(error) => {
                log::warn!("AW_ESP_ENUM_FAIL status={:?}", error.status());
                break;
            }
        }
    }

    root.reset_entry_readout().map_err(|error| error.status())?;
    // The Recovery Core picks the generation, verifies its image against the recorded digest
    // and, when nothing trustworthy can boot, takes over (rollback, diagnostics, power-off).
    let kernel_image = recovery::choose_kernel(&mut root)?;
    // Boot integrity IDS: the measured boot must replay to the TPM and match the previous boot;
    // any discrepancy is announced aloud. Then the chosen kernel is measured into PCR 9.
    measured::check(&mut root);
    measured::measure_kernel(&kernel_image);
    // Network stays closed unless the owner left a one-shot request (deny by default).
    net::on_request(&mut root);

    log::info!("AW_KERNEL_FILE_READ_OK bytes={}", kernel_image.len());

    let header = AwknImageHeader::parse(&kernel_image).map_err(|error| {
        log::error!("AW_KERNEL_IMAGE_HEADER_FAIL error={:?}", error);
        Status::LOAD_ERROR
    })?;
    log::info!(
        "AW_KERNEL_IMAGE_HEADER_OK base=0x{:x} file={} memory={} entry=0x{:x} bss={}",
        header.load_base,
        header.file_byte_len,
        header.memory_byte_len,
        header.entry_point,
        header.bss_byte_len
    );

    let load_address = usize::try_from(header.load_base).map_err(|_| Status::LOAD_ERROR)?;
    let allocation_len = usize::try_from(header.memory_byte_len).map_err(|_| Status::LOAD_ERROR)?;
    let pages = usize::try_from(header.page_count()).map_err(|_| Status::LOAD_ERROR)?;

    // The kernel is linked non-relocatable at `header.load_base`, so this
    // allocation must succeed at that exact address. There is deliberately no
    // fallback: loading elsewhere would corrupt every absolute reference in the
    // image instead of failing, which is the class of silent breakage this
    // header was introduced to end.
    let allocation = boot::allocate_pages(
        AllocateType::Address(header.load_base),
        MemoryType::LOADER_DATA,
        pages,
    )
    .map_err(|error| {
        log::error!(
            "AW_NATIVE_KERNEL_LOAD_FAIL reason=fixed_base_unavailable base=0x{:x} pages={} status={:?}",
            header.load_base,
            pages,
            error.status()
        );
        error.status()
    })?;
    debug_assert_eq!(allocation.as_ptr() as usize, load_address);

    // SAFETY: `allocation` owns `allocation_len` writable bytes starting at
    // `header.load_base`, which the header validated as page aligned and
    // non-overflowing. The whole allocation is zeroed first, so the BSS window
    // and the zero tail `objcopy` truncated from the file are both correct
    // before the file bytes (never longer than the allocation) are copied in.
    unsafe {
        core::ptr::write_bytes(allocation.as_ptr(), 0, allocation_len);
        core::ptr::copy_nonoverlapping(
            kernel_image.as_ptr(),
            allocation.as_ptr(),
            kernel_image.len(),
        );
    }

    log::info!(
        "AW_NATIVE_KERNEL_LOAD_OK address=0x{:x} bytes={} pages={} mode=fixed_base",
        load_address,
        kernel_image.len(),
        pages
    );

    Ok(LoadedKernel {
        entry_address: usize::try_from(header.entry_point).map_err(|_| Status::LOAD_ERROR)?,
        image_base: load_address,
        image_size: kernel_image.len(),
        allocation_size: allocation_len,
    })
}

fn normalize_final_memory_map<M: MemoryMap>(
    memory_map: &M,
    buffer_address: usize,
    capacity_entries: usize,
) -> Option<MemoryMapHandoff> {
    if memory_map.is_empty() || memory_map.len() > capacity_entries {
        return None;
    }

    let output = buffer_address as *mut MemoryDescriptorHandoff;
    for (index, source) in memory_map.entries().enumerate() {
        let descriptor = MemoryDescriptorHandoff {
            memory_type: source.ty.0,
            reserved: 0,
            physical_start: source.phys_start,
            page_count: source.page_count,
            attributes: source.att.bits(),
        };
        if !descriptor.is_valid() {
            return None;
        }

        // SAFETY: `output` points to the page-aligned LOADER_DATA allocation
        // reserved before ExitBootServices. `capacity_entries` was derived from
        // that allocation and the bounds check above guarantees this write is
        // within it. No allocator or boot service is used here.
        unsafe { output.add(index).write(descriptor) };
    }

    let entry_count = u32::try_from(memory_map.len()).ok()?;
    let descriptor_size = u32::try_from(core::mem::size_of::<MemoryDescriptorHandoff>()).ok()?;
    let byte_len = u64::from(entry_count).checked_mul(u64::from(descriptor_size))?;
    let handoff = MemoryMapHandoff {
        buffer_address: buffer_address as u64,
        byte_len,
        entry_count,
        descriptor_size,
    };
    handoff.is_valid().then_some(handoff)
}

#[entry]
fn main() -> Status {
    if uefi::helpers::init().is_err() {
        return Status::ABORTED;
    }

    log::info!("AW_BOOT_OK stage=uefi_init arch=x86_64");

    let loaded_kernel = match load_native_kernel() {
        Ok(kernel) => kernel,
        Err(status) => {
            log::error!("AW_NATIVE_KERNEL_LOAD_FAIL status={:?}", status);
            return status;
        }
    };

    let memory_map = match boot::memory_map(MemoryType::LOADER_DATA) {
        Ok(memory_map) => memory_map,
        Err(_) => {
            log::error!("AW_MEMORY_MAP_FAIL");
            return Status::DEVICE_ERROR;
        }
    };
    log::info!("AW_MEMORY_MAP_OK entries={}", memory_map.len());
    drop(memory_map);

    let acpi = system::with_config_table(|tables| {
        tables
            .iter()
            .find(|entry| entry.guid == ConfigTableEntry::ACPI2_GUID)
            .map(|entry| (entry.address as usize, 2_u8))
            .or_else(|| {
                tables
                    .iter()
                    .find(|entry| entry.guid == ConfigTableEntry::ACPI_GUID)
                    .map(|entry| (entry.address as usize, 1_u8))
            })
    });

    let Some((acpi_address, acpi_table_revision)) = acpi else {
        log::error!("AW_ACPI_FAIL reason=no_rsdp");
        return Status::NOT_FOUND;
    };
    log::info!(
        "AW_ACPI_OK table_revision={} rsdp=0x{:x}",
        acpi_table_revision,
        acpi_address
    );

    let rsdp = match validate_firmware_rsdp(acpi_address, acpi_table_revision) {
        Ok(rsdp) => rsdp,
        Err(error) => {
            log::error!("AW_ACPI_VALIDATE_FAIL error={:?}", error);
            return Status::COMPROMISED_DATA;
        }
    };
    log::info!(
        "AW_ACPI_VALIDATE_OK revision={} length={} rsdt=0x{:x} xsdt=0x{:x}",
        rsdp.revision,
        rsdp.length,
        rsdp.rsdt_address,
        rsdp.xsdt_address.unwrap_or(0)
    );

    let ecam = discover_pcie_ecam(&rsdp);

    let gop_handle = match boot::get_handle_for_protocol::<GraphicsOutput>() {
        Ok(handle) => handle,
        Err(_) => {
            log::error!("AW_GOP_FAIL reason=no_handle");
            return Status::NOT_FOUND;
        }
    };

    let mut gop = match boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle) {
        Ok(gop) => gop,
        Err(_) => {
            log::error!("AW_GOP_FAIL reason=open_protocol");
            return Status::DEVICE_ERROR;
        }
    };

    let mode = gop.current_mode_info();
    let (width, height) = mode.resolution();
    let stride_pixels = mode.stride();
    let uefi_pixel_format = mode.pixel_format();
    let handoff_pixel_format = match uefi_pixel_format {
        UefiPixelFormat::Rgb => HandoffPixelFormat::Rgb,
        UefiPixelFormat::Bgr => HandoffPixelFormat::Bgr,
        UefiPixelFormat::Bitmask => HandoffPixelFormat::Bitmask,
        UefiPixelFormat::BltOnly => HandoffPixelFormat::Unknown,
    };

    log::info!(
        "AW_GOP_OK width={} height={} stride={} format={:?}",
        width,
        height,
        stride_pixels,
        uefi_pixel_format
    );

    let framebuffer = if uefi_pixel_format == UefiPixelFormat::BltOnly {
        log::warn!("AW_FRAMEBUFFER_UNAVAILABLE reason=blt_only");
        None
    } else {
        let mut frame_buffer = gop.frame_buffer();
        let address = frame_buffer.as_mut_ptr() as usize;
        let size = frame_buffer.size();
        log::info!("AW_FRAMEBUFFER_OK address=0x{:x} size={}", address, size);
        Some(FramebufferHandoff {
            physical_address: address as u64,
            byte_len: size as u64,
            width: width as u32,
            height: height as u32,
            stride_pixels: stride_pixels as u32,
            pixel_format: handoff_pixel_format,
        })
    };

    // Accessibility before the operating system: with the display mode now known,
    // the native screen reader voices the boot screen on the visible console and
    // lets the user review it and continue by keyboard, while boot services (and
    // so the console and its keyboard) are still available. Unattended, it reads
    // the screen and continues on its own.
    screen_reader::run(width, height);

    let normalized_memory_map_buffer = match boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        NORMALIZED_MEMORY_MAP_PAGES,
    ) {
        Ok(buffer) => buffer,
        Err(error) => {
            log::error!("AW_MEMORY_MAP_BUFFER_FAIL status={:?}", error.status());
            return error.status();
        }
    };
    let normalized_memory_map_buffer_address = normalized_memory_map_buffer.as_ptr() as usize;
    let normalized_memory_map_capacity = NORMALIZED_MEMORY_MAP_PAGES * UEFI_PAGE_SIZE
        / core::mem::size_of::<MemoryDescriptorHandoff>();
    log::info!(
        "AW_MEMORY_MAP_BUFFER_OK address=0x{:x} pages={} capacity={}",
        normalized_memory_map_buffer_address,
        NORMALIZED_MEMORY_MAP_PAGES,
        normalized_memory_map_capacity
    );

    drop(gop);

    log::info!("AW_EXIT_BOOT_SERVICES_BEGIN");

    // SAFETY: All boot-services-backed protocol objects and temporary memory
    // maps have been dropped. The native kernel and normalized memory-map
    // buffer occupy LOADER_DATA pages that remain reserved across
    // ExitBootServices.
    let final_memory_map = unsafe { boot::exit_boot_services(None) };

    log::info!(
        "AW_EXIT_BOOT_SERVICES_OK entries={}",
        final_memory_map.len()
    );

    let memory_map_handoff = match normalize_final_memory_map(
        &final_memory_map,
        normalized_memory_map_buffer_address,
        normalized_memory_map_capacity,
    ) {
        Some(memory_map) => memory_map,
        None => {
            log::error!(
                "AW_MEMORY_MAP_NORMALIZE_FAIL entries={} capacity={}",
                final_memory_map.len(),
                normalized_memory_map_capacity
            );
            loop {
                core::hint::spin_loop();
            }
        }
    };
    log::info!(
        "AW_MEMORY_MAP_HANDOFF_OK entries={} bytes={} descriptor_size={}",
        memory_map_handoff.entry_count,
        memory_map_handoff.byte_len,
        memory_map_handoff.descriptor_size
    );

    let kernel_image_handoff = KernelImageHandoff {
        physical_address: loaded_kernel.image_base as u64,
        image_byte_len: loaded_kernel.image_size as u64,
        allocation_byte_len: loaded_kernel.allocation_size as u64,
    };
    if !kernel_image_handoff.is_valid() {
        log::error!("AW_KERNEL_IMAGE_HANDOFF_FAIL");
        loop {
            core::hint::spin_loop();
        }
    }
    log::info!(
        "AW_KERNEL_IMAGE_HANDOFF_OK address=0x{:x} image_bytes={} allocation_bytes={}",
        kernel_image_handoff.physical_address,
        kernel_image_handoff.image_byte_len,
        kernel_image_handoff.allocation_byte_len
    );

    let handoff = KernelHandoff::new(
        acpi_address as u64,
        kernel_image_handoff,
        memory_map_handoff,
        framebuffer,
        ecam.regions,
        ecam.count,
    );

    if let Err(error) = aw_kernel_core::enter(&handoff) {
        log::error!("AW_KERNEL_HANDOFF_FAIL error={:?}", error);
        loop {
            core::hint::spin_loop();
        }
    }

    log::info!(
        "AW_KERNEL_HANDOFF_OK magic=0x{:x} abi={} size={} memory_entries={} flags=0x{:x} ecam_regions={}",
        handoff.magic,
        handoff.abi_version,
        handoff.struct_size,
        handoff.memory_map.entry_count,
        handoff.flags,
        handoff.pcie_ecam_count
    );
    log::info!(
        "AW_NATIVE_KERNEL_TRANSFER address=0x{:x} entry=0x{:x} bytes={}",
        loaded_kernel.image_base,
        loaded_kernel.entry_address,
        loaded_kernel.image_size
    );

    type KernelEntry = extern "sysv64" fn(*const KernelHandoff) -> !;
    // SAFETY: The kernel image was placed at exactly the load base it was
    // linked against and its BSS window was zeroed, so every absolute reference
    // in the image resolves correctly. `entry_address` is the `_start` address
    // the validated `AWKN` header declares. The ABI is shared with the kernel
    // crate.
    let kernel_entry: KernelEntry = unsafe { core::mem::transmute(loaded_kernel.entry_address) };
    kernel_entry(core::ptr::addr_of!(handoff));
}
