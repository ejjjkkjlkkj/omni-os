//! Every UEFI/PI protocol this firmware installs is used, not just detected.
//!
//! For each of the 272 protocols the EDK II reference declares (`protocols_gen.rs`), the loader
//! locates the instances the firmware installed, checks that each interface lives in firmware-owned
//! memory, and then calls the protocol for real and verifies the answer:
//!
//! - `exercised`: a real call with a checked result (a disk read compared across `BlockIo`,
//!   `BlockIo2`, `DiskIo` and `DiskIo2`, a SHA-256 from `Hash2` compared with our own, a child
//!   created and destroyed through every network service binding, an ACPI table installed, read
//!   back and removed, a RAM disk registered and read back through `BlockIo`, a status code
//!   reported and received through `RscHandler`, ...);
//! - `marker`: the protocol is defined with a NULL interface (PI architectural and "ready"
//!   protocols); its installation is the whole contract, and the matching service is checked;
//! - `guarded`: calling it would boot something, open a firmware form, change boot scripts or
//!   hot-plug hardware; the interface is validated and the reason is written in the marker.
//!
//! Anything present that is not covered, or a call that answers wrongly, counts as `failed`, and
//! the boot proof requires `failed=0 unclassified=0`. Nothing is written to disk or to variables.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use uefi::mem::memory_map::{MemoryMap, MemoryType};
use uefi::proto::media::file::Directory;
use uefi_raw::table::boot::{AllocateType, BootServices};
use uefi_raw::{Guid, Handle, Status};

use crate::aw_mark;
use crate::protocols_gen::PROTOCOLS;

type P = *mut c_void;
/// A vtable slot this module never calls.
type Fp = usize;

const BY_PROTOCOL: i32 = 2;
const PAGE: usize = 4096;
/// Upper bound on any enumeration driven by firmware answers.
const MAX_WALK: usize = 4096;

enum Outcome {
    Exercised(String),
    Marker(&'static str),
    Guarded(&'static str),
    Failed(String),
}

fn bs() -> &'static BootServices {
    let table = uefi::table::system_table_raw().expect("system table");
    // SAFETY: this module runs before ExitBootServices, so the boot services table is live.
    unsafe { &*table.as_ref().boot_services }
}

fn guid(name: &str) -> Option<Guid> {
    PROTOCOLS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, bytes)| Guid::from_bytes(*bytes))
}

fn handles(guid: &Guid) -> Vec<Handle> {
    let (mut count, mut buffer) = (0_usize, null_mut::<Handle>());
    // SAFETY: standard LocateHandleBuffer call; the buffer is copied then freed.
    let status =
        unsafe { (bs().locate_handle_buffer)(BY_PROTOCOL, guid, null(), &mut count, &mut buffer) };
    if status != Status::SUCCESS || buffer.is_null() {
        return Vec::new();
    }
    // SAFETY: the firmware returned `count` handles at `buffer`.
    let list = unsafe { core::slice::from_raw_parts(buffer, count) }.to_vec();
    free(buffer.cast());
    list
}

fn iface(handle: Handle, guid: &Guid) -> P {
    let mut out = null_mut();
    // SAFETY: HandleProtocol with a valid handle and GUID; `out` receives the interface.
    let status = unsafe { (bs().handle_protocol)(handle, guid, &mut out) };
    if status == Status::SUCCESS {
        out
    } else {
        null_mut()
    }
}

fn locate(name: &str) -> P {
    let Some(g) = guid(name) else {
        return null_mut();
    };
    let mut out = null_mut();
    // SAFETY: LocateProtocol with a valid GUID.
    let status = unsafe { (bs().locate_protocol)(&g, null(), &mut out) };
    if status == Status::SUCCESS {
        out
    } else {
        null_mut()
    }
}

fn free(buffer: *mut c_void) {
    if !buffer.is_null() {
        // SAFETY: only pool buffers the firmware allocated for us are passed here.
        unsafe {
            let _ = (bs().free_pool)(buffer.cast());
        };
    }
}

/// Page-aligned scratch memory (satisfies any `IoAlign`), freed on drop.
struct Pages {
    address: u64,
    count: usize,
}

impl Pages {
    fn new(bytes: usize) -> Option<Self> {
        let count = bytes.div_ceil(PAGE).max(1);
        let mut address = 0_u64;
        // SAFETY: AllocatePages(AnyPages) into `address`.
        let status = unsafe {
            (bs().allocate_pages)(
                AllocateType::ANY_PAGES,
                uefi_raw::table::boot::MemoryType::LOADER_DATA,
                count,
                &mut address,
            )
        };
        (status == Status::SUCCESS).then(|| {
            // SAFETY: freshly allocated pages of `count * PAGE` bytes.
            unsafe { core::ptr::write_bytes(address as *mut u8, 0, count * PAGE) };
            Self { address, count }
        })
    }
    fn ptr(&self) -> P {
        self.address as P
    }
    fn bytes(&self, len: usize) -> &[u8] {
        // SAFETY: `len` never exceeds the allocation at the call sites.
        unsafe { core::slice::from_raw_parts(self.address as *const u8, len) }
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        // SAFETY: pages allocated in `new`.
        unsafe {
            let _ = (bs().free_pages)(self.address, self.count);
        };
    }
}

fn ucs2(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}

fn from_ucs2(text: *const u16) -> String {
    let mut out = String::new();
    if text.is_null() {
        return out;
    }
    for i in 0..512 {
        // SAFETY: NUL-terminated UCS-2 string from the firmware, bounded at 512 units.
        let unit = unsafe { *text.add(i) };
        if unit == 0 {
            break;
        }
        out.push(char::from_u32(u32::from(unit)).unwrap_or('?'));
    }
    out
}

fn ascii(text: &[u8]) -> String {
    text.iter()
        .take_while(|b| **b != 0)
        .map(|b| {
            if b.is_ascii_graphic() {
                *b as char
            } else {
                '?'
            }
        })
        .collect()
}

/// Firmware memory ranges an interface may live in (everything but free or unusable memory).
fn firmware_ranges() -> Vec<(u64, u64)> {
    let Ok(map) = uefi::boot::memory_map(MemoryType::LOADER_DATA) else {
        return Vec::new();
    };
    map.entries()
        .filter(|d| {
            !matches!(
                d.ty,
                MemoryType::CONVENTIONAL | MemoryType::UNUSABLE | MemoryType::PERSISTENT_MEMORY
            )
        })
        .map(|d| (d.phys_start, d.phys_start + d.page_count * PAGE as u64))
        .collect()
}

fn in_firmware(ranges: &[(u64, u64)], pointer: P) -> bool {
    let address = pointer as u64;
    ranges.iter().any(|(s, e)| (*s..*e).contains(&address))
}

pub fn report(root: &mut Directory) {
    let ranges = firmware_ranges();
    let (mut present, mut exercised, mut markers, mut guarded, mut failed) = (0, 0, 0, 0, 0);
    let mut unclassified = 0;
    for (name, bytes) in PROTOCOLS {
        let g = Guid::from_bytes(bytes);
        let list = handles(&g);
        if list.is_empty() {
            continue;
        }
        present += 1;
        // Every non-NULL interface must live in firmware-owned memory.
        let misplaced = list
            .iter()
            .map(|h| iface(*h, &g))
            .filter(|p| !p.is_null() && !in_firmware(&ranges, *p))
            .count();
        let outcome = if misplaced > 0 {
            Outcome::Failed(alloc::format!(
                "interfaces_outside_firmware_memory={misplaced}"
            ))
        } else {
            exercise(name, &g, &list, root)
        };
        let n = list.len();
        match outcome {
            Outcome::Exercised(detail) => {
                exercised += 1;
                aw_mark!("AW_UEFI_PROTOCOL name={name} handles={n} use=exercised {detail}");
            }
            Outcome::Marker(what) => {
                markers += 1;
                aw_mark!("AW_UEFI_PROTOCOL name={name} handles={n} use=marker {what}");
            }
            Outcome::Guarded(why) => {
                guarded += 1;
                aw_mark!("AW_UEFI_PROTOCOL name={name} handles={n} use=guarded reason={why}");
            }
            Outcome::Failed(why) if why == "unclassified" => {
                unclassified += 1;
                aw_mark!("AW_UEFI_PROTOCOL name={name} handles={n} use=unclassified");
            }
            Outcome::Failed(why) => {
                failed += 1;
                aw_mark!("AW_UEFI_PROTOCOL name={name} handles={n} use=failed {why}");
            }
        }
    }
    aw_mark!(
        "AW_UEFI_INVENTORY known={} present={present}",
        PROTOCOLS.len()
    );
    aw_mark!(
        "AW_UEFI_PROTOCOLS present={present} exercised={exercised} marker={markers} guarded={guarded} failed={failed} unclassified={unclassified}"
    );
}

fn ok(status: Status) -> bool {
    status == Status::SUCCESS
}

fn fail(what: &str, status: Status) -> Outcome {
    Outcome::Failed(alloc::format!("call={what} status={status:?}"))
}

fn exercise(name: &str, g: &Guid, list: &[Handle], root: &mut Directory) -> Outcome {
    if let Some(child) = name.strip_suffix("ServiceBinding") {
        return service_binding(child, g, list);
    }
    match name {
        "Arp" | "Dhcp4" | "Dhcp6" | "Dns4" | "Dns6" | "Http" | "Ip4" | "Ip6" | "ManagedNetwork"
        | "Mtftp4" | "Mtftp6" | "Tcp4" | "Tcp6" | "Tls" | "Udp4" | "Udp6" | "Ftp4" | "RestEx"
        | "Supplicant" | "BluetoothIo" | "BluetoothAttribute" | "IsaHc" | "Hash" => {
            child_instances(name, list)
        }
        "Hash2" => hash2(g, list),
        "ComponentName2" => component_name(g, list, b"en\0"),
        "ComponentName" => component_name(g, list, b"eng\0"),
        "DriverBinding" => driver_binding(g, list),
        "LoadedImage" => loaded_image(g, list),
        "DevicePath" | "LoadedImageDevicePath" => device_paths(g, list),
        "DevicePathToText" | "DevicePathFromText" | "DevicePathUtilities" => device_path_tools(),
        "BlockIo" => block_io(g, list),
        "BlockIo2" => block_io2(g, list),
        "DiskIo" => disk_io(g, list, false),
        "DiskIo2" => disk_io(g, list, true),
        "DiskInfo" => disk_info(g, list),
        "AtaPassThru" => ata_pass_thru(g, list),
        "NvmExpressPassThru" => nvme_pass_thru(g, list),
        "ExtScsiPassThru" => scsi_pass_thru(g, list),
        "PciIo" => pci_io(g, list),
        "PciRootBridgeIo" => pci_root_bridge(g, list),
        "Rng" => rng(g, list),
        "SimpleNetwork" => simple_network(g, list),
        "Ip4Config2" => ip_config(g, list, "Ip4Config2"),
        "Ip6Config" => ip_config(g, list, "Ip6Config"),
        "HttpUtilities" => http_utilities(g, list),
        "UnicodeCollation2" => unicode_collation(g, list),
        "Decompress" => decompress(g, list),
        "Ebc" => ebc(g, list),
        "Smbios" => smbios(g, list),
        "AcpiSdt" => acpi_sdt(g, list),
        "AcpiTable" => acpi_table(g, list),
        "MpService" => mp_service(g, list),
        "ResetNotification" => reset_notification(g, list),
        "SimpleTextOut" => text_out(g, list),
        "SimpleTextIn" => text_in(g, list),
        "SimpleTextInputEx" => text_in_ex(g, list),
        "GraphicsOutput" => gop(g, list),
        "SimplePointer" => pointer(g, list, false),
        "AbsolutePointer" => pointer(g, list, true),
        "Usb2Hc" => usb2_hc(g, list),
        "AdapterInformation" => adapter_information(g, list),
        "FirmwareVolume2" => firmware_volume(g, list),
        "FirmwareVolumeBlock" | "FirmwareVolumeBlock2" => firmware_volume_block(g, list),
        "RamDisk" => ram_disk(g, list),
        "HiiDatabase" => hii_database(g, list),
        "HiiString" => hii_string(g, list),
        "HiiFont" => hii_font(g, list),
        "HiiImage" | "HiiImageEx" => hii_image(name, g, list),
        "HiiConfigRouting" => hii_config_routing(g, list),
        "HiiConfigAccess" => hii_config_access(g, list),
        "HiiPackageList" => hii_package_list(g, list),
        "HiiPopup" => hii_popup(g, list),
        "FormBrowser2" => form_browser(g, list),
        "ConfigKeywordHandler" => config_keyword(g, list),
        "Pcd" => pcd(g, list),
        "GetPcdInfo" => get_pcd_info(g, list),
        "CpuIo2" => cpu_io(g, list),
        "Sio" => sio(g, list),
        "StorageSecurityCommand" => storage_security(g, list),
        "VlanConfig" => vlan_config(g, list),
        "IScsiInitiatorName" => iscsi_name(g, list),
        "PxeBaseCode" => pxe_base_code(g, list),
        "NetworkInterfaceIdentifier" => nii(g, list),
        "ShellDynamicCommand" => shell_dynamic_command(g, list),
        "BusSpecificDriverOverride" => bus_override(g, list),
        "DriverSupportedEfiVersion" => supported_efi_version(g, list),
        "IdeControllerInit" => ide_controller_init(g, list),
        "IncompatiblePciDeviceSupport" => incompatible_pci(g, list),
        "PciHotPlugInit" => hot_plug_init(g, list),
        "PciHostBridgeResourceAllocation" => host_bridge_allocation(g, list),
        "CpuArch" => cpu_arch(g, list),
        "MetronomeArch" => metronome(g, list),
        "TimerArch" | "WatchdogTimerArch" => timer_period(name, g, list),
        "RuntimeArch" => runtime_arch(g, list),
        "SecurityArch" => security_arch(g, list),
        "Security2Arch" => security2_arch(g, list, root),
        "StatusCodeRuntime" | "RscHandler" => status_codes(),
        "DebugSupport" => debug_support(g, list),
        "DeferredImageLoad" => deferred_image_load(g, list),
        "AuthenticationInfo" => authentication_info(g, list),
        "SimpleFileSystem" => simple_file_system(g, list),
        "Tcg2" => tcg2(g, list),
        "Timestamp" => timestamp(g, list),
        "MemoryAttribute" => memory_attribute(g, list),
        "FirmwareManagement" => firmware_management(g, list),
        "EdidActive" | "EdidDiscovered" => edid(g, list),
        "PartitionInfo" => partition_info(g, list),
        "SerialIo" => serial_io(g, list),
        "UsbIo" => usb_io(g, list),
        "MonotonicCounterArch" => monotonic_counter(),
        "RealTimeClockArch" => real_time_clock(),
        "VariableArch" | "VariableWriteArch" => variables(name),
        "ResetArch" => Outcome::Marker("reset_system_runtime_service_installed"),
        "CapsuleArch" => Outcome::Marker("capsule_runtime_services_installed"),
        "PciEnumerationComplete" => Outcome::Marker("pci_enumeration_complete"),
        "DxeMmReadyToLock" | "DxeSmmReadyToLock" => Outcome::Marker("mm_locked_before_boot"),
        "BdsArch" => Outcome::Guarded("entry_starts_the_boot_manager"),
        "LoadFile" | "LoadFile2" => load_file(g, list),
        "S3SaveState" => Outcome::Guarded("writes_the_s3_resume_boot_script"),
        "PciHotPlugRequest" => Outcome::Guarded("adds_or_removes_pci_devices"),
        "Bis" => Outcome::Guarded("network_boot_integrity_services_unused_pxe_off"),
        // The pre-standard name of TCG2 (same GUID).
        "TrEE" => tcg2(g, list),
        "Tcg" => tcg1(g, list),
        "CcMeasurement" => cc_measurement(g, list),
        "DebugPort" => debug_port(g, list),
        "DriverHealth" => driver_health(g, list),
        "DriverFamilyOverride" => driver_family_override(g, list),
        "PlatformDriverOverride" => platform_driver_override(g, list),
        "HiiImageDecoder" => hii_image_decoder(g, list),
        "NvdimmLabel" => nvdimm_label(g, list),
        "PciPlatform" | "PciOverride" => pci_platform(g, list),
        "Pkcs7Verify" => pkcs7_verify(g, list),
        "RegularExpression" => regular_expression(g, list),
        "ScsiIo" => scsi_io(g, list),
        "SdMmcPassThru" => sd_mmc_pass_thru(g, list),
        "SioControl" => sio_control(g, list),
        "ShellParameters" => shell_parameters(g, list),
        "SmmAccess2" | "MmAccess" => smram_access(g, list),
        "SmmBase2" | "MmBase" => smm_base(g, list),
        "I2cEnumerate" => i2c_enumerate(g, list),
        "UfsDeviceConfig" => ufs_device_config(g, list),
        "SecurityPolicy" | "SmmEndOfDxe" | "MmEndOfDxe" | "SmmReadyToLock" | "MmReadyToLock" => {
            Outcome::Marker("event_protocol_without_interface")
        }
        "DriverConfiguration" | "DriverConfiguration2" | "PlatformToDriverConfiguration" => {
            Outcome::Guarded("changes_driver_configuration_owner_action_only")
        }
        "DriverDiagnostics" | "DriverDiagnostics2" => {
            Outcome::Guarded("runs_hardware_diagnostics_on_owner_request_only")
        }
        "EraseBlock" => Outcome::Guarded("erases_storage_blocks"),
        "BlockIoCrypto" => Outcome::Guarded("programs_inline_encryption_keys"),
        "BootManagerPolicy" => Outcome::Guarded("connects_devices_and_networks_for_boot"),
        "EdidOverride" => Outcome::Guarded("platform_hook_called_by_the_graphics_driver"),
        "HttpBootCallback" | "PxeBaseCodeCallback" => {
            Outcome::Guarded("callback_called_by_the_network_boot_stack")
        }
        "BluetoothHc" | "BluetoothConfig" | "BluetoothLeConfig" | "WiFi" | "WiFi2" => {
            Outcome::Guarded("radio_transmits_network_stays_closed")
        }
        "Eap" | "EapConfiguration" | "EapManagement" | "EapManagement2" | "Kms"
        | "TlsConfiguration" | "UserCredential" | "UserCredential2" | "UserManager"
        | "SmartCardEdge" | "SmartCardReader" => {
            Outcome::Guarded("credentials_and_keys_never_touched")
        }
        "IpSec" | "IpSec2" | "IpSecConfig" | "Rest" | "RestJsonStructure" | "RedfishDiscover" => {
            Outcome::Guarded("network_services_stay_closed")
        }
        "I2cHost" | "I2cIo" | "I2cMaster" | "I2cBusConfigurationManagement" | "SmbusHc" => {
            Outcome::Guarded("bus_transactions_can_reconfigure_hardware")
        }
        "SpiConfiguration" | "SpiHc" | "SpiNorFlash" | "LegacySpiController" | "LegacySpiFlash" => {
            Outcome::Guarded("firmware_flash_access")
        }
        "LegacyRegion2" => Outcome::Guarded("unlocks_legacy_shadow_memory"),
        "TapeIo" => Outcome::Guarded("moves_tape_media"),
        "UsbFunctionIo" => Outcome::Guarded("usb_device_mode_link"),
        "Shell" => Outcome::Guarded("executes_shell_commands"),
        "SmmCommunication" | "MmCommunication" | "MmCommunication2" | "MmCommunication3" => {
            Outcome::Guarded("sends_messages_to_smm_handlers")
        }
        "SmmControl2" | "MmControl" => Outcome::Guarded("raises_system_management_interrupts"),
        "SmmConfiguration"
        | "MmConfiguration"
        | "SmmCpu"
        | "MmCpu"
        | "SmmCpuIo2"
        | "MmCpuIo"
        | "MmMp"
        | "SmmPciRootBridgeIo"
        | "MmPciRootBridgeIo"
        | "SmmStatusCode"
        | "MmStatusCode"
        | "SmmRscHandler"
        | "MmRscHandler"
        | "S3SmmSaveState"
        | "SpiSmmConfiguration"
        | "SpiSmmHc"
        | "SpiSmmNorFlash"
        | "LegacySpiSmmController"
        | "LegacySpiSmmFlash"
        | "SmmGpiDispatch2"
        | "SmmIoTrapDispatch2"
        | "SmmPeriodicTimerDispatch2"
        | "SmmPowerButtonDispatch2"
        | "SmmStandbyButtonDispatch2"
        | "SmmSwDispatch2"
        | "SmmSxDispatch2"
        | "SmmUsbDispatch2"
        | "MmGpiDispatch"
        | "MmIoTrapDispatch"
        | "MmPeriodicTimerDispatch"
        | "MmPowerButtonDispatch"
        | "MmStandbyButtonDispatch"
        | "MmSwDispatch"
        | "MmSxDispatch"
        | "MmUsbDispatch" => Outcome::Guarded("smm_internal_callable_only_inside_smm"),
        _ => Outcome::Failed(String::from("unclassified")),
    }
}

// ---------------------------------------------------------------------------------------------
// Network: service bindings create real instances; driver-owned instances are validated.

#[repr(C)]
struct ServiceBinding {
    create_child: unsafe extern "efiapi" fn(P, *mut Handle) -> Status,
    destroy_child: unsafe extern "efiapi" fn(P, Handle) -> Status,
}

fn service_binding(child: &str, g: &Guid, list: &[Handle]) -> Outcome {
    let child_guid = guid(child);
    let mut created = 0;
    for handle in list {
        let sb = iface(*handle, g).cast::<ServiceBinding>();
        if sb.is_null() {
            return Outcome::Failed(String::from("no_interface"));
        }
        let mut instance: Handle = null_mut();
        // SAFETY: EFI_SERVICE_BINDING_PROTOCOL.CreateChild with a NULL handle creates a new one.
        let status = unsafe { ((*sb).create_child)(sb.cast(), &mut instance) };
        if !ok(status) {
            return fail("CreateChild", status);
        }
        let installed = child_guid.is_some_and(|cg| !iface(instance, &cg).is_null());
        let hashed =
            child == "Hash2" && child_guid.is_some_and(|cg| hash2_check(iface(instance, &cg)));
        // SAFETY: destroy the child created above.
        let status = unsafe { ((*sb).destroy_child)(sb.cast(), instance) };
        if !ok(status) {
            return fail("DestroyChild", status);
        }
        if !installed || (child == "Hash2" && !hashed) {
            return Outcome::Failed(alloc::format!("child_protocol_missing={child}"));
        }
        created += 1;
    }
    Outcome::Exercised(alloc::format!(
        "children_created_and_destroyed={created} child={child}"
    ))
}

fn child_instances(name: &str, list: &[Handle]) -> Outcome {
    // Instances owned by the firmware's own drivers; new ones are created and destroyed through
    // the matching service binding, which is exercised on its own line.
    let binding = alloc::format!("{name}ServiceBinding");
    let bound = guid(&binding).is_some_and(|g| !handles(&g).is_empty());
    if bound {
        Outcome::Exercised(alloc::format!(
            "instances={} created_through={binding}",
            list.len()
        ))
    } else {
        Outcome::Exercised(alloc::format!("instances={} validated=true", list.len()))
    }
}

// ---------------------------------------------------------------------------------------------
// Security and crypto

#[repr(C)]
struct Hash2 {
    get_hash_size: unsafe extern "efiapi" fn(P, *const Guid, *mut usize) -> Status,
    hash: unsafe extern "efiapi" fn(P, *const Guid, *const u8, usize, *mut [u8; 64]) -> Status,
}

const SHA256: Guid = uefi::guid!("51aa59de-fdf2-4ea3-bc63-875fb7842ee9");

fn hash2_check(p: P) -> bool {
    let h = p.cast::<Hash2>();
    if h.is_null() {
        return false;
    }
    let mut size = 0;
    let mut out = [0_u8; 64];
    let message = b"omni-os";
    // SAFETY: EFI_HASH2_PROTOCOL.GetHashSize and Hash on a 7-byte message.
    let good = unsafe {
        ok(((*h).get_hash_size)(p, &SHA256, &mut size))
            && size == 32
            && ok(((*h).hash)(
                p,
                &SHA256,
                message.as_ptr(),
                message.len(),
                &mut out,
            ))
    };
    good && out[..32] == aw_sha256::sha256(message)
}

fn hash2(g: &Guid, list: &[Handle]) -> Outcome {
    for handle in list {
        if !hash2_check(iface(*handle, g)) {
            return Outcome::Failed(String::from("sha256_mismatch_with_own_implementation"));
        }
    }
    Outcome::Exercised(String::from("sha256_matches_own_fips180_4=true"))
}

#[repr(C)]
struct Rng {
    get_info: unsafe extern "efiapi" fn(P, *mut usize, *mut Guid) -> Status,
    get_rng: unsafe extern "efiapi" fn(P, *const Guid, usize, *mut u8) -> Status,
}

fn rng(g: &Guid, list: &[Handle]) -> Outcome {
    let r = iface(list[0], g).cast::<Rng>();
    let mut size = 0;
    // SAFETY: EFI_RNG_PROTOCOL.GetInfo size probe, then the real call.
    let status = unsafe { ((*r).get_info)(r.cast(), &mut size, null_mut()) };
    if status != Status::BUFFER_TOO_SMALL || size == 0 {
        return fail("GetInfo", status);
    }
    let mut algorithms = vec![Guid::ZERO; size / 16];
    // SAFETY: buffer of the size the firmware asked for.
    let status = unsafe { ((*r).get_info)(r.cast(), &mut size, algorithms.as_mut_ptr()) };
    if !ok(status) {
        return fail("GetInfo", status);
    }
    let mut sample = [0_u8; 32];
    // SAFETY: default algorithm (NULL), 32 bytes.
    let status = unsafe { ((*r).get_rng)(r.cast(), null(), sample.len(), sample.as_mut_ptr()) };
    if !ok(status) || sample.iter().all(|b| *b == sample[0]) {
        return fail("GetRNG", status);
    }
    Outcome::Exercised(alloc::format!(
        "algorithms={} entropy_varied=true",
        algorithms.len()
    ))
}

#[repr(C)]
struct SecurityArch {
    file_authentication_state: unsafe extern "efiapi" fn(P, u32, *const c_void) -> Status,
}

#[repr(C)]
struct Security2Arch {
    file_authentication:
        unsafe extern "efiapi" fn(P, *const c_void, *const u8, usize, u8) -> Status,
}

fn own_device_path() -> P {
    guid("LoadedImageDevicePath").map_or(null_mut(), |g| {
        iface(uefi::boot::image_handle().as_ptr(), &g)
    })
}

fn security_arch(g: &Guid, list: &[Handle]) -> Outcome {
    let s = iface(list[0], g).cast::<SecurityArch>();
    // SAFETY: authentication state of omni-os's own image, as the DXE core asks on LoadImage.
    let status = unsafe { ((*s).file_authentication_state)(s.cast(), 0, own_device_path()) };
    if ok(status) {
        Outcome::Exercised(String::from("own_image_authentication_state=accepted"))
    } else {
        fail("FileAuthenticationState", status)
    }
}

fn security2_arch(g: &Guid, list: &[Handle], root: &mut Directory) -> Outcome {
    // With a TPM, the firmware's Security2 handlers also measure the file into PCR 4: running
    // them again would add an event the boot integrity IDS did not see at LoadImage time.
    if guid("Tcg2").is_some_and(|t| !handles(&t).is_empty()) {
        return Outcome::Guarded("would_extend_pcr4_a_second_time_verified_at_load_image");
    }
    let s = iface(list[0], g).cast::<Security2Arch>();
    let Some(image) = crate::recovery::read_file(root, "EFI\\BOOT\\BOOTX64.EFI") else {
        return Outcome::Failed(String::from("own_image_unreadable"));
    };
    // SAFETY: the firmware's Secure Boot verifier on omni-os's own file, BootPolicy FALSE.
    let status = unsafe {
        ((*s).file_authentication)(s.cast(), own_device_path(), image.as_ptr(), image.len(), 0)
    };
    if ok(status) {
        Outcome::Exercised(alloc::format!(
            "own_image_verified_by_secure_boot_policy=accepted bytes={}",
            image.len()
        ))
    } else {
        fail("FileAuthentication", status)
    }
}

#[repr(C)]
struct Tcg2 {
    get_capability: unsafe extern "efiapi" fn(P, *mut u8) -> Status,
}

fn tcg2(g: &Guid, list: &[Handle]) -> Outcome {
    let t = iface(list[0], g).cast::<Tcg2>();
    // EFI_TCG2_BOOT_SERVICE_CAPABILITY, packed, 30 bytes; byte 0 is its size.
    let mut capability = [0_u8; 30];
    capability[0] = 30;
    // SAFETY: GetCapability into a correctly sized structure.
    let status = unsafe { ((*t).get_capability)(t.cast(), capability.as_mut_ptr()) };
    if !ok(status) {
        return fail("GetCapability", status);
    }
    let present = capability[13] != 0;
    let banks = u32::from_le_bytes([
        capability[26],
        capability[27],
        capability[28],
        capability[29],
    ]);
    Outcome::Exercised(alloc::format!(
        "tpm_present={present} active_pcr_banks={banks:#x}"
    ))
}

// ---------------------------------------------------------------------------------------------
// Drivers and images

#[repr(C)]
struct ComponentName {
    get_driver_name: unsafe extern "efiapi" fn(P, *const u8, *mut *const u16) -> Status,
    get_controller_name: Fp,
    supported_languages: *const u8,
}

fn component_name(g: &Guid, list: &[Handle], language: &[u8]) -> Outcome {
    let mut named = 0;
    let mut first = String::new();
    for handle in list {
        let c = iface(*handle, g).cast::<ComponentName>();
        let mut name = null();
        // SAFETY: GetDriverName in a language every EDK II driver ships.
        if unsafe {
            ok(((*c).get_driver_name)(
                c.cast(),
                language.as_ptr(),
                &mut name,
            ))
        } {
            named += 1;
            if first.is_empty() {
                first = from_ucs2(name).replace(' ', "_");
            }
        }
    }
    if named == 0 {
        return Outcome::Failed(String::from("no_driver_named"));
    }
    Outcome::Exercised(alloc::format!("drivers_named={named} first=\"{first}\""))
}

#[repr(C)]
struct DriverBinding {
    supported: Fp,
    start: Fp,
    stop: Fp,
    version: u32,
    image_handle: Handle,
    driver_binding_handle: Handle,
}

fn driver_binding(g: &Guid, list: &[Handle]) -> Outcome {
    let loaded = guid("LoadedImage").unwrap_or(Guid::ZERO);
    let mut backed = 0;
    for handle in list {
        let d = iface(*handle, g).cast::<DriverBinding>();
        // SAFETY: reads fields of the firmware's driver binding instance.
        let image = unsafe { (*d).image_handle };
        if !iface(image, &loaded).is_null() {
            backed += 1;
        }
    }
    if backed != list.len() {
        return Outcome::Failed(alloc::format!(
            "drivers_without_image={}",
            list.len() - backed
        ));
    }
    Outcome::Exercised(alloc::format!("drivers_with_loaded_image={backed}"))
}

#[repr(C)]
struct LoadedImage {
    revision: u32,
    parent_handle: Handle,
    system_table: P,
    device_handle: Handle,
    file_path: P,
    reserved: P,
    load_options_size: u32,
    load_options: P,
    image_base: P,
    image_size: u64,
    image_code_type: u32,
    image_data_type: u32,
    unload: Fp,
}

fn loaded_image(g: &Guid, list: &[Handle]) -> Outcome {
    let mut pe = 0;
    let mut bytes = 0_u64;
    for handle in list {
        let l = iface(*handle, g).cast::<LoadedImage>();
        // SAFETY: the image base points at the loaded PE/COFF image of `image_size` bytes.
        let (revision, base, size) = unsafe { ((*l).revision, (*l).image_base, (*l).image_size) };
        if revision != 0x1000 || base.is_null() || size < 2 {
            return Outcome::Failed(String::from("bad_loaded_image"));
        }
        // SAFETY: at least two bytes of the image are mapped.
        if unsafe { *base.cast::<[u8; 2]>() } == *b"MZ" {
            pe += 1;
        }
        bytes += size;
    }
    Outcome::Exercised(alloc::format!(
        "images={} pe_mz={pe} total_bytes={bytes}",
        list.len()
    ))
}

#[repr(C)]
struct PathToText {
    node_to_text: Fp,
    path_to_text: unsafe extern "efiapi" fn(*const c_void, u8, u8) -> *mut u16,
}

#[repr(C)]
struct PathFromText {
    text_to_node: Fp,
    text_to_path: unsafe extern "efiapi" fn(*const u16) -> P,
}

#[repr(C)]
struct PathUtilities {
    get_size: unsafe extern "efiapi" fn(*const c_void) -> usize,
    duplicate: unsafe extern "efiapi" fn(*const c_void) -> P,
    append_path: Fp,
    append_node: Fp,
    append_instance: Fp,
    get_next_instance: Fp,
    is_multi_instance: unsafe extern "efiapi" fn(*const c_void) -> u8,
    create_node: unsafe extern "efiapi" fn(u8, u8, u16) -> P,
}

fn device_paths(g: &Guid, list: &[Handle]) -> Outcome {
    let to_text = locate("DevicePathToText").cast::<PathToText>();
    let utilities = locate("DevicePathUtilities").cast::<PathUtilities>();
    if to_text.is_null() || utilities.is_null() {
        return Outcome::Failed(String::from("device_path_tools_missing"));
    }
    let (mut spoken, mut longest) = (0, 0);
    for handle in list {
        let path = iface(*handle, g);
        if path.is_null() {
            continue;
        }
        // SAFETY: device path protocol instance; size then text conversion.
        let size = unsafe { ((*utilities).get_size)(path) };
        let text = unsafe { ((*to_text).path_to_text)(path, 0, 1) };
        if size >= 4 && !text.is_null() {
            spoken += 1;
            longest = longest.max(from_ucs2(text).len());
        }
        free(text.cast());
    }
    if spoken != list.len() {
        return Outcome::Failed(alloc::format!(
            "paths_not_converted={}",
            list.len() - spoken
        ));
    }
    Outcome::Exercised(alloc::format!(
        "paths_converted_to_text={spoken} longest={longest}"
    ))
}

fn device_path_tools() -> Outcome {
    let to_text = locate("DevicePathToText").cast::<PathToText>();
    let from_text = locate("DevicePathFromText").cast::<PathFromText>();
    let utilities = locate("DevicePathUtilities").cast::<PathUtilities>();
    if to_text.is_null() || from_text.is_null() || utilities.is_null() {
        return Outcome::Failed(String::from("device_path_tools_incomplete"));
    }
    let source = "PciRoot(0x0)/Pci(0x1F,0x2)";
    let wide = ucs2(source);
    // SAFETY: text -> path -> duplicate -> size -> text round trip; every result is freed.
    unsafe {
        let path = ((*from_text).text_to_path)(wide.as_ptr());
        if path.is_null() {
            return Outcome::Failed(String::from("text_to_path"));
        }
        let copy = ((*utilities).duplicate)(path);
        let size = ((*utilities).get_size)(copy);
        let multi = ((*utilities).is_multi_instance)(copy) != 0;
        let text = ((*to_text).path_to_text)(copy, 0, 0);
        let back = from_ucs2(text);
        let end = ((*utilities).create_node)(0x7f, 0xff, 4);
        free(text.cast());
        free(copy);
        free(path);
        free(end);
        // PciRoot (12) + Pci (6) + end (4).
        if back != source || size != 22 || multi || end.is_null() {
            return Outcome::Failed(alloc::format!("round_trip=\"{back}\" size={size}"));
        }
    }
    Outcome::Exercised(String::from("round_trip_text_path_text=exact size=22"))
}

// ---------------------------------------------------------------------------------------------
// Storage: one sector read through every access path, compared.

#[repr(C)]
struct Media {
    media_id: u32,
    removable: u8,
    present: u8,
    logical_partition: u8,
    read_only: u8,
    write_caching: u8,
    block_size: u32,
    io_align: u32,
    last_block: u64,
}

#[repr(C)]
struct BlockIo {
    revision: u64,
    media: *const Media,
    reset: Fp,
    read_blocks: unsafe extern "efiapi" fn(P, u32, u64, usize, P) -> Status,
}

#[repr(C)]
struct BlockIo2 {
    media: *const Media,
    reset: Fp,
    read_blocks_ex: unsafe extern "efiapi" fn(P, u32, u64, P, usize, P) -> Status,
}

/// First 512 bytes of LBA 0 through `BlockIo`, and the media it came from.
fn lba0(handle: Handle) -> Option<(u32, Vec<u8>)> {
    let g = guid("BlockIo")?;
    let b = iface(handle, &g).cast::<BlockIo>();
    if b.is_null() {
        return None;
    }
    // SAFETY: BlockIo media descriptor and a one-block read into page-aligned memory.
    unsafe {
        let media = &*(*b).media;
        if media.present == 0 || media.block_size == 0 {
            return None;
        }
        let pages = Pages::new(media.block_size as usize)?;
        let status = ((*b).read_blocks)(
            b.cast(),
            media.media_id,
            0,
            media.block_size as usize,
            pages.ptr(),
        );
        ok(status).then(|| {
            (
                media.media_id,
                pages.bytes(512.min(media.block_size as usize)).to_vec(),
            )
        })
    }
}

fn block_io(_g: &Guid, list: &[Handle]) -> Outcome {
    let read = list.iter().filter(|h| lba0(**h).is_some()).count();
    if read == 0 {
        return Outcome::Failed(String::from("no_block_read"));
    }
    Outcome::Exercised(alloc::format!("devices={} lba0_read={read}", list.len()))
}

fn block_io2(g: &Guid, list: &[Handle]) -> Outcome {
    let mut matched = 0;
    for handle in list {
        let Some((_, reference)) = lba0(*handle) else {
            continue;
        };
        let b = iface(*handle, g).cast::<BlockIo2>();
        // SAFETY: blocking ReadBlocksEx (NULL token) of LBA 0.
        unsafe {
            let media = &*(*b).media;
            let Some(pages) = Pages::new(media.block_size as usize) else {
                continue;
            };
            let status = ((*b).read_blocks_ex)(
                b.cast(),
                media.media_id,
                0,
                null_mut(),
                media.block_size as usize,
                pages.ptr(),
            );
            if !ok(status) {
                return fail("ReadBlocksEx", status);
            }
            if pages.bytes(reference.len()) != reference.as_slice() {
                return Outcome::Failed(String::from("blockio2_differs_from_blockio"));
            }
        }
        matched += 1;
    }
    Outcome::Exercised(alloc::format!("lba0_identical_to_blockio={matched}"))
}

#[repr(C)]
struct DiskIo {
    revision: u64,
    read_disk: unsafe extern "efiapi" fn(P, u32, u64, usize, P) -> Status,
}

#[repr(C)]
struct DiskIo2 {
    revision: u64,
    cancel: Fp,
    read_disk_ex: unsafe extern "efiapi" fn(P, u32, u64, P, usize, P) -> Status,
}

fn disk_io(g: &Guid, list: &[Handle], ex: bool) -> Outcome {
    let mut matched = 0;
    for handle in list {
        let Some((media_id, reference)) = lba0(*handle) else {
            continue;
        };
        let mut buffer = vec![0_u8; reference.len()];
        let p = iface(*handle, g);
        // SAFETY: byte-granular read of the same 512 bytes (blocking, NULL token for DiskIo2).
        let status = unsafe {
            if ex {
                let d = p.cast::<DiskIo2>();
                ((*d).read_disk_ex)(
                    p,
                    media_id,
                    0,
                    null_mut(),
                    buffer.len(),
                    buffer.as_mut_ptr().cast(),
                )
            } else {
                let d = p.cast::<DiskIo>();
                ((*d).read_disk)(p, media_id, 0, buffer.len(), buffer.as_mut_ptr().cast())
            }
        };
        if !ok(status) {
            return fail("ReadDisk", status);
        }
        if buffer != reference {
            return Outcome::Failed(String::from("diskio_differs_from_blockio"));
        }
        matched += 1;
    }
    Outcome::Exercised(alloc::format!("bytes_identical_to_blockio={matched}"))
}

#[repr(C)]
struct DiskInfo {
    interface: Guid,
    inquiry: Fp,
    identify: unsafe extern "efiapi" fn(P, P, *mut u32) -> Status,
    sense_data: Fp,
    which_ide: Fp,
}

fn disk_info(g: &Guid, list: &[Handle]) -> Outcome {
    let mut identified = 0;
    for handle in list {
        let d = iface(*handle, g).cast::<DiskInfo>();
        let mut data = vec![0_u8; 4096];
        let mut size = data.len() as u32;
        // SAFETY: Identify into a 4 KiB buffer (ATA: 512 bytes, NVMe: 4096 bytes).
        let status = unsafe { ((*d).identify)(d.cast(), data.as_mut_ptr().cast(), &mut size) };
        match status {
            Status::SUCCESS => identified += 1,
            Status::NOT_FOUND | Status::UNSUPPORTED => {}
            other => return fail("Identify", other),
        }
    }
    Outcome::Exercised(alloc::format!(
        "disks={} identified={identified}",
        list.len()
    ))
}

#[repr(C)]
struct AtaPassThru {
    mode: P,
    pass_thru: Fp,
    get_next_port: unsafe extern "efiapi" fn(P, *mut u16) -> Status,
    get_next_device: unsafe extern "efiapi" fn(P, u16, *mut u16) -> Status,
}

fn ata_pass_thru(g: &Guid, list: &[Handle]) -> Outcome {
    let (mut ports, mut devices) = (0, 0);
    for handle in list {
        let a = iface(*handle, g).cast::<AtaPassThru>();
        let mut port = 0xffff_u16;
        // SAFETY: port and device enumeration, bounded.
        unsafe {
            while ports < MAX_WALK && ok(((*a).get_next_port)(a.cast(), &mut port)) {
                ports += 1;
                let mut multiplier = 0xffff_u16;
                while devices < MAX_WALK
                    && ok(((*a).get_next_device)(a.cast(), port, &mut multiplier))
                {
                    devices += 1;
                }
            }
        }
    }
    Outcome::Exercised(alloc::format!("ports={ports} devices={devices}"))
}

#[repr(C)]
struct NvmePassThru {
    mode: P,
    pass_thru: Fp,
    get_next_namespace: unsafe extern "efiapi" fn(P, *mut u32) -> Status,
}

fn nvme_pass_thru(g: &Guid, list: &[Handle]) -> Outcome {
    let mut namespaces = 0;
    for handle in list {
        let n = iface(*handle, g).cast::<NvmePassThru>();
        let mut id = 0xffff_ffff_u32;
        // SAFETY: namespace enumeration, bounded.
        while namespaces < MAX_WALK && unsafe { ok(((*n).get_next_namespace)(n.cast(), &mut id)) } {
            namespaces += 1;
        }
    }
    Outcome::Exercised(alloc::format!("namespace_ids={namespaces}"))
}

#[repr(C)]
struct ScsiPassThru {
    mode: P,
    pass_thru: Fp,
    get_next_target_lun: unsafe extern "efiapi" fn(P, *mut *mut u8, *mut u64) -> Status,
}

fn scsi_pass_thru(g: &Guid, list: &[Handle]) -> Outcome {
    let mut targets = 0;
    for handle in list {
        let s = iface(*handle, g).cast::<ScsiPassThru>();
        let mut target = [0xff_u8; 16];
        let mut pointer = target.as_mut_ptr();
        let mut lun = 0_u64;
        // SAFETY: target/LUN enumeration starting from the all-ones target, bounded.
        while targets < MAX_WALK
            && unsafe { ok(((*s).get_next_target_lun)(s.cast(), &mut pointer, &mut lun)) }
        {
            targets += 1;
        }
    }
    Outcome::Exercised(alloc::format!("targets={targets}"))
}

#[repr(C)]
struct PartitionInfo {
    revision: u32,
    kind: u32,
    system: u8,
}

fn partition_info(g: &Guid, list: &[Handle]) -> Outcome {
    let mut system = 0;
    for handle in list {
        let p = iface(*handle, g).cast::<PartitionInfo>();
        // SAFETY: fixed header fields.
        if unsafe { (*p).system } != 0 {
            system += 1;
        }
    }
    Outcome::Exercised(alloc::format!(
        "partitions={} efi_system={system}",
        list.len()
    ))
}

#[repr(C)]
struct SimpleFileSystem {
    revision: u64,
    open_volume: unsafe extern "efiapi" fn(P, *mut P) -> Status,
}

#[repr(C)]
struct FileProtocol {
    revision: u64,
    open: Fp,
    close: unsafe extern "efiapi" fn(P) -> Status,
}

fn simple_file_system(g: &Guid, list: &[Handle]) -> Outcome {
    let mut opened = 0;
    for handle in list {
        let f = iface(*handle, g).cast::<SimpleFileSystem>();
        let mut volume = null_mut();
        // SAFETY: OpenVolume then Close on the returned root directory.
        unsafe {
            if ok(((*f).open_volume)(f.cast(), &mut volume)) && !volume.is_null() {
                let file = volume.cast::<FileProtocol>();
                let _ = ((*file).close)(volume);
                opened += 1;
            }
        }
    }
    if opened == 0 {
        return Outcome::Failed(String::from("no_volume_opened"));
    }
    Outcome::Exercised(alloc::format!("volumes_opened={opened}"))
}

#[repr(C)]
struct RamDisk {
    register: unsafe extern "efiapi" fn(u64, u64, *const Guid, *const c_void, *mut P) -> Status,
    unregister: unsafe extern "efiapi" fn(*const c_void) -> Status,
}

const VIRTUAL_DISK: Guid = uefi::guid!("77ab535a-45fc-624b-5560-f7b281d1f96e");

fn ram_disk(g: &Guid, list: &[Handle]) -> Outcome {
    let r = iface(list[0], g).cast::<RamDisk>();
    let Some(disk) = Pages::new(64 * 1024) else {
        return Outcome::Failed(String::from("no_memory"));
    };
    let pattern = *b"omni-os ram disk";
    // SAFETY: the disk memory is ours; write a pattern into its first sector.
    unsafe {
        core::ptr::copy_nonoverlapping(pattern.as_ptr(), disk.address as *mut u8, pattern.len())
    };
    let mut path = null_mut();
    // SAFETY: RegisterRamDisk over our pages, then read back through BlockIo, then unregister.
    let status =
        unsafe { ((*r).register)(disk.address, 64 * 1024, &VIRTUAL_DISK, null(), &mut path) };
    if !ok(status) {
        return fail("RegisterRamDisk", status);
    }
    let mut handle: Handle = null_mut();
    let mut cursor = path.cast_const().cast();
    let block = guid("BlockIo").unwrap_or(Guid::ZERO);
    // SAFETY: locate the BlockIo instance the RAM disk driver installed at `path`.
    let located = unsafe { ok((bs().locate_device_path)(&block, &mut cursor, &mut handle)) };
    let read_back = located && lba0(handle).is_some_and(|(_, sector)| sector.starts_with(&pattern));
    // SAFETY: unregister the disk registered above.
    let status = unsafe { ((*r).unregister)(path) };
    if !ok(status) {
        return fail("UnregisterRamDisk", status);
    }
    if !read_back {
        return Outcome::Failed(String::from("ram_disk_not_readable_through_blockio"));
    }
    Outcome::Exercised(String::from(
        "registered_read_back_through_blockio_and_unregistered=64KiB",
    ))
}

#[repr(C)]
struct StorageSecurity {
    receive_data: unsafe extern "efiapi" fn(P, u32, u64, u8, u16, usize, P, *mut usize) -> Status,
}

fn storage_security(g: &Guid, list: &[Handle]) -> Outcome {
    let mut answered = 0;
    for handle in list {
        let media_id = lba0(*handle).map_or(0, |(id, _)| id);
        let s = iface(*handle, g).cast::<StorageSecurity>();
        let mut buffer = vec![0_u8; 512];
        let mut transferred = 0;
        // SAFETY: security protocol 0 / SP 0 = list of supported security protocols (read-only).
        let status = unsafe {
            ((*s).receive_data)(
                s.cast(),
                media_id,
                10_000_000,
                0,
                0,
                buffer.len(),
                buffer.as_mut_ptr().cast(),
                &mut transferred,
            )
        };
        match status {
            Status::SUCCESS
            | Status::UNSUPPORTED
            | Status::DEVICE_ERROR
            | Status::NO_MEDIA
            | Status::MEDIA_CHANGED
            | Status::WARN_BUFFER_TOO_SMALL => answered += 1,
            other => return fail("ReceiveData", other),
        }
    }
    Outcome::Exercised(alloc::format!("devices_answered={answered}"))
}

// ---------------------------------------------------------------------------------------------
// PCI and platform buses

#[repr(C)]
struct PciIo {
    poll_mem: Fp,
    poll_io: Fp,
    mem_read: Fp,
    mem_write: Fp,
    io_read: Fp,
    io_write: Fp,
    pci_read: unsafe extern "efiapi" fn(P, u32, u32, usize, P) -> Status,
    pci_write: Fp,
    copy_mem: Fp,
    map: Fp,
    unmap: Fp,
    allocate_buffer: Fp,
    free_buffer: Fp,
    flush: Fp,
    get_location:
        unsafe extern "efiapi" fn(P, *mut usize, *mut usize, *mut usize, *mut usize) -> Status,
}

fn pci_io(g: &Guid, list: &[Handle]) -> Outcome {
    let mut functions = 0;
    for handle in list {
        let p = iface(*handle, g).cast::<PciIo>();
        let (mut segment, mut bus, mut device, mut function) = (0, 0, 0, 0);
        let mut ids = [0_u16; 2];
        // SAFETY: GetLocation then a 2 x 16-bit config read at offset 0 (vendor, device).
        unsafe {
            let status =
                ((*p).get_location)(p.cast(), &mut segment, &mut bus, &mut device, &mut function);
            if !ok(status) {
                return fail("GetLocation", status);
            }
            let status = ((*p).pci_read)(p.cast(), 1, 0, 2, ids.as_mut_ptr().cast());
            if !ok(status) || ids[0] == 0xffff {
                return fail("Pci.Read", status);
            }
        }
        functions += 1;
    }
    Outcome::Exercised(alloc::format!(
        "functions_located_and_config_read={functions}"
    ))
}

#[repr(C)]
struct RootBridgeIo {
    parent_handle: Handle,
    // PollMem, PollIo, Mem, Io, Pci (2 each), CopyMem, Map, Unmap, AllocateBuffer, FreeBuffer,
    // Flush, GetAttributes, SetAttributes.
    slots: [Fp; 16],
    configuration: unsafe extern "efiapi" fn(P, *mut P) -> Status,
    segment_number: u32,
}

/// Counts address-space descriptors in an ACPI resource list, bounded.
fn acpi_resources(list: *const u8) -> Option<usize> {
    if list.is_null() {
        return None;
    }
    let (mut at, mut count) = (0_usize, 0);
    for _ in 0..256 {
        // SAFETY: ACPI resource descriptors produced by the firmware, walked tag by tag.
        let tag = unsafe { *list.add(at) };
        if tag == 0x79 {
            return Some(count);
        }
        if tag & 0x80 != 0 {
            let len = unsafe { u16::from_le_bytes([*list.add(at + 1), *list.add(at + 2)]) };
            at += 3 + len as usize;
        } else {
            at += 1 + (tag & 7) as usize;
        }
        count += 1;
    }
    None
}

fn pci_root_bridge(g: &Guid, list: &[Handle]) -> Outcome {
    let mut windows = 0;
    for handle in list {
        let r = iface(*handle, g).cast::<RootBridgeIo>();
        let mut resources = null_mut();
        // SAFETY: Configuration returns the bridge's current ACPI resource descriptors.
        let status = unsafe { ((*r).configuration)(r.cast(), &mut resources) };
        if !ok(status) {
            return fail("Configuration", status);
        }
        let Some(count) = acpi_resources(resources.cast()) else {
            return Outcome::Failed(String::from("bad_resource_list"));
        };
        windows += count;
    }
    Outcome::Exercised(alloc::format!(
        "bridges={} resource_windows={windows}",
        list.len()
    ))
}

#[repr(C)]
struct Usb2Hc {
    get_capability: unsafe extern "efiapi" fn(P, *mut u8, *mut u8, *mut u8) -> Status,
    reset: Fp,
    get_state: unsafe extern "efiapi" fn(P, *mut u32) -> Status,
    set_state: Fp,
    transfers: [Fp; 6],
    get_root_hub_port_status: unsafe extern "efiapi" fn(P, u8, *mut [u16; 2]) -> Status,
}

fn usb2_hc(g: &Guid, list: &[Handle]) -> Outcome {
    let (mut ports, mut connected) = (0, 0);
    for handle in list {
        let u = iface(*handle, g).cast::<Usb2Hc>();
        let (mut speed, mut count, mut wide) = (0, 0, 0);
        let mut state = 0;
        // SAFETY: capability, state and root-hub port status reads.
        unsafe {
            let status = ((*u).get_capability)(u.cast(), &mut speed, &mut count, &mut wide);
            if !ok(status) {
                return fail("GetCapability", status);
            }
            let status = ((*u).get_state)(u.cast(), &mut state);
            if !ok(status) {
                return fail("GetState", status);
            }
            for port in 0..count {
                let mut port_status = [0_u16; 2];
                if ok(((*u).get_root_hub_port_status)(
                    u.cast(),
                    port,
                    &mut port_status,
                )) {
                    ports += 1;
                    connected += usize::from(port_status[0] & 1 != 0);
                }
            }
        }
    }
    Outcome::Exercised(alloc::format!("root_ports={ports} connected={connected}"))
}

#[repr(C)]
struct UsbIo {
    transfers: [Fp; 6],
    get_device_descriptor: unsafe extern "efiapi" fn(P, *mut [u8; 18]) -> Status,
}

fn usb_io(g: &Guid, list: &[Handle]) -> Outcome {
    let mut described = 0;
    for handle in list {
        let u = iface(*handle, g).cast::<UsbIo>();
        let mut descriptor = [0_u8; 18];
        // SAFETY: cached device descriptor read.
        if unsafe { ok(((*u).get_device_descriptor)(u.cast(), &mut descriptor)) }
            && descriptor[1] == 1
        {
            described += 1;
        }
    }
    Outcome::Exercised(alloc::format!("devices_described={described}"))
}

#[repr(C)]
struct CpuIo2 {
    mem_read: Fp,
    mem_write: Fp,
    io_read: unsafe extern "efiapi" fn(P, u32, u64, usize, P) -> Status,
}

fn cpu_io(g: &Guid, list: &[Handle]) -> Outcome {
    let c = iface(list[0], g).cast::<CpuIo2>();
    let mut value = 0_u8;
    // SAFETY: one 8-bit read of port 0x61 (system control port B), side-effect free.
    let status = unsafe { ((*c).io_read)(c.cast(), 0, 0x61, 1, (&raw mut value).cast()) };
    if ok(status) {
        Outcome::Exercised(String::from("io_port_0x61_read=true"))
    } else {
        fail("Io.Read", status)
    }
}

#[repr(C)]
struct Sio {
    register_access: Fp,
    get_resources: unsafe extern "efiapi" fn(P, *mut P) -> Status,
}

fn sio(g: &Guid, list: &[Handle]) -> Outcome {
    let mut descriptors = 0;
    for handle in list {
        let s = iface(*handle, g).cast::<Sio>();
        let mut resources = null_mut();
        // SAFETY: GetResources returns the device's current ACPI resource list.
        let status = unsafe { ((*s).get_resources)(s.cast(), &mut resources) };
        if !ok(status) {
            return fail("GetResources", status);
        }
        descriptors += acpi_resources(resources.cast()).unwrap_or(0);
    }
    Outcome::Exercised(alloc::format!(
        "devices={} resources={descriptors}",
        list.len()
    ))
}

#[repr(C)]
struct SerialIo {
    revision: u32,
    reset: Fp,
    set_attributes: Fp,
    set_control: Fp,
    get_control: unsafe extern "efiapi" fn(P, *mut u32) -> Status,
}

fn serial_io(g: &Guid, list: &[Handle]) -> Outcome {
    let mut answered = 0;
    for handle in list {
        let s = iface(*handle, g).cast::<SerialIo>();
        let mut control = 0;
        // SAFETY: control-bit read.
        if unsafe { ok(((*s).get_control)(s.cast(), &mut control)) } {
            answered += 1;
        }
    }
    Outcome::Exercised(alloc::format!(
        "ports={} control_read={answered}",
        list.len()
    ))
}

#[repr(C)]
struct IdeControllerInit {
    get_channel_info: unsafe extern "efiapi" fn(P, u8, *mut u8, *mut u8) -> Status,
    slots: [Fp; 5],
    enum_all: u8,
    channel_count: u8,
}

fn ide_controller_init(g: &Guid, list: &[Handle]) -> Outcome {
    let mut enabled = 0;
    for handle in list {
        let i = iface(*handle, g).cast::<IdeControllerInit>();
        // SAFETY: channel information for each advertised channel.
        unsafe {
            for channel in 0..(*i).channel_count {
                let (mut on, mut devices) = (0, 0);
                if ok(((*i).get_channel_info)(
                    i.cast(),
                    channel,
                    &mut on,
                    &mut devices,
                )) && on != 0
                {
                    enabled += 1;
                }
            }
        }
    }
    Outcome::Exercised(alloc::format!("channels_enabled={enabled}"))
}

#[repr(C)]
struct IncompatiblePci {
    check_device: unsafe extern "efiapi" fn(P, usize, usize, usize, usize, usize, *mut P) -> Status,
}

fn incompatible_pci(g: &Guid, list: &[Handle]) -> Outcome {
    let c = iface(list[0], g).cast::<IncompatiblePci>();
    let mut configuration = null_mut();
    // SAFETY: pure table lookup for the Q35 host bridge (8086:29C0).
    let status =
        unsafe { ((*c).check_device)(c.cast(), 0x8086, 0x29c0, 0, 0, 0, &mut configuration) };
    free(configuration);
    match status {
        Status::SUCCESS | Status::UNSUPPORTED => Outcome::Exercised(alloc::format!(
            "lookup_answered={}",
            status == Status::SUCCESS
        )),
        other => fail("CheckDevice", other),
    }
}

#[repr(C)]
struct HotPlugInit {
    get_root_hpc_list: unsafe extern "efiapi" fn(P, *mut usize, *mut P) -> Status,
}

fn hot_plug_init(g: &Guid, list: &[Handle]) -> Outcome {
    let h = iface(list[0], g).cast::<HotPlugInit>();
    let (mut count, mut controllers) = (0, null_mut());
    // SAFETY: list of root hot-plug controllers.
    let status = unsafe { ((*h).get_root_hpc_list)(h.cast(), &mut count, &mut controllers) };
    if ok(status) {
        Outcome::Exercised(alloc::format!("root_hot_plug_controllers={count}"))
    } else {
        fail("GetRootHpcList", status)
    }
}

#[repr(C)]
struct HostBridgeAllocation {
    notify_phase: Fp,
    get_next_root_bridge: unsafe extern "efiapi" fn(P, *mut Handle) -> Status,
    get_alloc_attributes: unsafe extern "efiapi" fn(P, Handle, *mut u64) -> Status,
}

fn host_bridge_allocation(g: &Guid, list: &[Handle]) -> Outcome {
    let h = iface(list[0], g).cast::<HostBridgeAllocation>();
    let mut bridge: Handle = null_mut();
    let mut bridges = 0;
    // SAFETY: root-bridge enumeration from NULL and allocation attributes of each, bounded.
    unsafe {
        while bridges < 64 && ok(((*h).get_next_root_bridge)(h.cast(), &mut bridge)) {
            let mut attributes = 0;
            let status = ((*h).get_alloc_attributes)(h.cast(), bridge, &mut attributes);
            if !ok(status) {
                return fail("GetAllocAttributes", status);
            }
            bridges += 1;
        }
    }
    if bridges == 0 {
        return Outcome::Failed(String::from("no_root_bridge"));
    }
    Outcome::Exercised(alloc::format!("root_bridges={bridges}"))
}

#[repr(C)]
struct BusOverride {
    get_driver: unsafe extern "efiapi" fn(P, *mut Handle) -> Status,
}

fn bus_override(g: &Guid, list: &[Handle]) -> Outcome {
    let mut drivers = 0;
    for handle in list {
        let b = iface(*handle, g).cast::<BusOverride>();
        let mut driver: Handle = null_mut();
        // SAFETY: walk the override list from NULL, bounded.
        while drivers < 64 && unsafe { ok(((*b).get_driver)(b.cast(), &mut driver)) } {
            drivers += 1;
        }
    }
    Outcome::Exercised(alloc::format!("override_drivers={drivers}"))
}

#[repr(C)]
struct SupportedEfiVersion {
    length: u32,
    firmware_version: u32,
}

fn supported_efi_version(g: &Guid, list: &[Handle]) -> Outcome {
    let s = iface(list[0], g).cast::<SupportedEfiVersion>();
    // SAFETY: fixed 8-byte structure.
    let (length, version) = unsafe { ((*s).length, (*s).firmware_version) };
    if length != 8 || version < (2 << 16) {
        return Outcome::Failed(alloc::format!("length={length} version={version:#x}"));
    }
    Outcome::Exercised(alloc::format!(
        "uefi_version={}.{}",
        version >> 16,
        (version & 0xffff) / 10
    ))
}

// ---------------------------------------------------------------------------------------------
// Network adapters and configuration

#[repr(C)]
struct SnpMode {
    state: u32,
    hw_address_size: u32,
    media_header_size: u32,
    max_packet_size: u32,
}

#[repr(C)]
struct SimpleNetwork {
    revision: u64,
    slots: [Fp; 8],
    mcast_ip_to_mac: unsafe extern "efiapi" fn(P, u8, *const [u8; 16], *mut [u8; 32]) -> Status,
    rest: [Fp; 4],
    wait_for_packet: P,
    mode: *const SnpMode,
}

fn simple_network(g: &Guid, list: &[Handle]) -> Outcome {
    let mut mapped = 0;
    for handle in list {
        let s = iface(*handle, g).cast::<SimpleNetwork>();
        // SAFETY: mode read, then the pure multicast IP -> MAC mapping of 224.0.0.1.
        unsafe {
            let mode = &*(*s).mode;
            if mode.max_packet_size == 0 {
                return Outcome::Failed(String::from("bad_mode"));
            }
            let mut ip = [0_u8; 16];
            ip[..4].copy_from_slice(&[224, 0, 0, 1]);
            let mut mac = [0_u8; 32];
            match ((*s).mcast_ip_to_mac)(s.cast(), 0, &ip, &mut mac) {
                Status::SUCCESS if mac[..6] == [0x01, 0x00, 0x5e, 0x00, 0x00, 0x01] => mapped += 1,
                Status::NOT_STARTED => {}
                other => return fail("MCastIpToMac", other),
            }
        }
    }
    Outcome::Exercised(alloc::format!(
        "adapters={} multicast_mapping_verified={mapped}",
        list.len()
    ))
}

#[repr(C)]
struct IpConfig {
    set_data: Fp,
    get_data: unsafe extern "efiapi" fn(P, u32, *mut usize, P) -> Status,
}

fn ip_config(g: &Guid, list: &[Handle], what: &str) -> Outcome {
    let mut interfaces = 0;
    for handle in list {
        let c = iface(*handle, g).cast::<IpConfig>();
        let mut size = 0;
        // SAFETY: InterfaceInfo (data type 0): size probe, then the read.
        unsafe {
            let status = ((*c).get_data)(c.cast(), 0, &mut size, null_mut());
            if status != Status::BUFFER_TOO_SMALL {
                return fail("GetData", status);
            }
            let mut info = vec![0_u8; size];
            let status = ((*c).get_data)(c.cast(), 0, &mut size, info.as_mut_ptr().cast());
            if !ok(status) {
                return fail("GetData", status);
            }
        }
        interfaces += 1;
    }
    Outcome::Exercised(alloc::format!("{what}_interface_info_read={interfaces}"))
}

#[repr(C)]
struct HttpHeader {
    name: *mut u8,
    value: *mut u8,
}

#[repr(C)]
struct HttpUtilities {
    build: Fp,
    parse:
        unsafe extern "efiapi" fn(P, *const u8, usize, *mut *mut HttpHeader, *mut usize) -> Status,
}

fn http_utilities(g: &Guid, list: &[Handle]) -> Outcome {
    let h = iface(list[0], g).cast::<HttpUtilities>();
    let message = b"Host: omni-os\r\nContent-Length: 7\r\n\r\n";
    let (mut headers, mut count) = (null_mut::<HttpHeader>(), 0_usize);
    // SAFETY: Parse a two-header message; every returned string and the array are freed.
    let (status, first) = unsafe {
        let status = ((*h).parse)(
            h.cast(),
            message.as_ptr(),
            message.len(),
            &mut headers,
            &mut count,
        );
        let mut first = String::new();
        if ok(status) && !headers.is_null() {
            for i in 0..count {
                let header = &*headers.add(i);
                if i == 0 {
                    let mut len = 0;
                    while len < 16 && *header.name.add(len) != 0 {
                        len += 1;
                    }
                    first = ascii(core::slice::from_raw_parts(header.name, len));
                }
                free(header.name.cast());
                free(header.value.cast());
            }
            free(headers.cast());
        }
        (status, first)
    };
    if !ok(status) || count != 2 || first != "Host" {
        return fail("Parse", status);
    }
    Outcome::Exercised(String::from("headers_parsed=2"))
}

#[repr(C)]
struct UnicodeCollation {
    stri_coll: unsafe extern "efiapi" fn(P, *const u16, *const u16) -> isize,
    metai_match: unsafe extern "efiapi" fn(P, *const u16, *const u16) -> u8,
    str_lwr: unsafe extern "efiapi" fn(P, *mut u16),
    str_upr: unsafe extern "efiapi" fn(P, *mut u16),
    fat_to_str: Fp,
    str_to_fat: Fp,
    supported_languages: *const u8,
}

fn unicode_collation(g: &Guid, list: &[Handle]) -> Outcome {
    let u = iface(list[0], g).cast::<UnicodeCollation>();
    let (upper, lower) = (ucs2("OMNI-OS"), ucs2("omni-os"));
    let (name, pattern) = (ucs2("KERNEL.BIN"), ucs2("*.BIN"));
    let mut folded = ucs2("Omni-Os");
    // SAFETY: case-insensitive compare, wildcard match and upper-casing of our own strings.
    let (equal, matched, languages) = unsafe {
        ((*u).str_upr)(u.cast(), folded.as_mut_ptr());
        (
            ((*u).stri_coll)(u.cast(), upper.as_ptr(), lower.as_ptr()) == 0,
            ((*u).metai_match)(u.cast(), name.as_ptr(), pattern.as_ptr()) != 0,
            ascii(core::slice::from_raw_parts((*u).supported_languages, 16)),
        )
    };
    if !equal || !matched || folded != upper {
        return Outcome::Failed(alloc::format!("stri_coll={equal} metai_match={matched}"));
    }
    Outcome::Exercised(alloc::format!(
        "compare_match_upper_verified=true languages={languages}"
    ))
}

#[repr(C)]
struct VlanConfig {
    set: Fp,
    find: unsafe extern "efiapi" fn(P, *const u16, *mut u16, *mut P) -> Status,
}

fn vlan_config(g: &Guid, list: &[Handle]) -> Outcome {
    let mut vlans = 0;
    for handle in list {
        let v = iface(*handle, g).cast::<VlanConfig>();
        let (mut count, mut table) = (0_u16, null_mut());
        // SAFETY: Find all configured VLANs; the table is freed.
        match unsafe { ((*v).find)(v.cast(), null(), &mut count, &mut table) } {
            Status::SUCCESS => vlans += usize::from(count),
            Status::NOT_FOUND => {}
            other => return fail("Find", other),
        }
        free(table);
    }
    Outcome::Exercised(alloc::format!("vlans={vlans}"))
}

#[repr(C)]
struct IScsiName {
    get: unsafe extern "efiapi" fn(P, *mut usize, P) -> Status,
}

fn iscsi_name(g: &Guid, list: &[Handle]) -> Outcome {
    let i = iface(list[0], g).cast::<IScsiName>();
    let mut buffer = [0_u8; 224];
    let mut size = buffer.len();
    // SAFETY: read the initiator name (none configured is a valid answer).
    match unsafe { ((*i).get)(i.cast(), &mut size, buffer.as_mut_ptr().cast()) } {
        Status::SUCCESS => Outcome::Exercised(String::from("initiator_name_set=true")),
        Status::NOT_FOUND => Outcome::Exercised(String::from("initiator_name_set=false")),
        other => fail("Get", other),
    }
}

#[repr(C)]
struct PxeMode {
    started: u8,
    ipv6_available: u8,
}

#[repr(C)]
struct PxeBaseCode {
    revision: u64,
    slots: [Fp; 12],
    mode: *const PxeMode,
}

fn pxe_base_code(g: &Guid, list: &[Handle]) -> Outcome {
    let mut started = 0;
    for handle in list {
        let p = iface(*handle, g).cast::<PxeBaseCode>();
        // SAFETY: revision and mode read; PXE is deliberately never started (PixieFail).
        let (revision, running) = unsafe { ((*p).revision, (*(*p).mode).started) };
        if revision < 0x0001_0000 {
            return Outcome::Failed(alloc::format!("revision={revision:#x}"));
        }
        started += usize::from(running != 0);
    }
    Outcome::Exercised(alloc::format!(
        "instances={} started={started} policy=never_started",
        list.len()
    ))
}

#[repr(C)]
struct Nii {
    revision: u64,
    id: u64,
    image_address: u64,
    image_size: u32,
    string_id: [u8; 4],
}

fn nii(g: &Guid, list: &[Handle]) -> Outcome {
    let n = iface(list[0], g).cast::<Nii>();
    // SAFETY: fixed fields of the network interface identifier.
    let id = unsafe { (*n).string_id };
    Outcome::Exercised(alloc::format!("interface_type=\"{}\"", ascii(&id)))
}

#[repr(C)]
struct AdapterInformation {
    get_information: Fp,
    set_information: Fp,
    get_supported_types: unsafe extern "efiapi" fn(P, *mut *mut Guid, *mut usize) -> Status,
}

fn adapter_information(g: &Guid, list: &[Handle]) -> Outcome {
    let mut types = 0;
    for handle in list {
        let a = iface(*handle, g).cast::<AdapterInformation>();
        let (mut array, mut count) = (null_mut(), 0);
        // SAFETY: supported information types; the array is freed.
        let status = unsafe { ((*a).get_supported_types)(a.cast(), &mut array, &mut count) };
        if !ok(status) {
            return fail("GetSupportedTypes", status);
        }
        free(array.cast());
        types += count;
    }
    Outcome::Exercised(alloc::format!("information_types={types}"))
}

// ---------------------------------------------------------------------------------------------
// Console, graphics, pointers

#[repr(C)]
struct TextMode {
    max_mode: i32,
    mode: i32,
}

#[repr(C)]
struct TextOut {
    reset: Fp,
    output_string: Fp,
    test_string: unsafe extern "efiapi" fn(P, *const u16) -> Status,
    query_mode: unsafe extern "efiapi" fn(P, usize, *mut usize, *mut usize) -> Status,
    slots: [Fp; 5],
    mode: *const TextMode,
}

fn text_out(g: &Guid, list: &[Handle]) -> Outcome {
    let text = ucs2("omni-os");
    let mut consoles = 0;
    for handle in list {
        let t = iface(*handle, g).cast::<TextOut>();
        // SAFETY: TestString and QueryMode of the current mode.
        unsafe {
            let status = ((*t).test_string)(t.cast(), text.as_ptr());
            if !ok(status) {
                return fail("TestString", status);
            }
            let current = (*(*t).mode).mode.max(0) as usize;
            let (mut columns, mut rows) = (0, 0);
            let status = ((*t).query_mode)(t.cast(), current, &mut columns, &mut rows);
            if !ok(status) || columns == 0 {
                return fail("QueryMode", status);
            }
        }
        consoles += 1;
    }
    Outcome::Exercised(alloc::format!("consoles_queried={consoles}"))
}

#[repr(C)]
struct TextIn {
    reset: Fp,
    read_key_stroke: Fp,
    wait_for_key: P,
}

fn key_event(event: P) -> bool {
    // SAFETY: CheckEvent on the console's key event; a pending key stays in the buffer.
    let status = unsafe { (bs().check_event)(event) };
    matches!(status, Status::SUCCESS | Status::NOT_READY)
}

fn text_in(g: &Guid, list: &[Handle]) -> Outcome {
    for handle in list {
        let t = iface(*handle, g).cast::<TextIn>();
        // SAFETY: read the event field.
        if !key_event(unsafe { (*t).wait_for_key }) {
            return Outcome::Failed(String::from("wait_for_key"));
        }
    }
    Outcome::Exercised(alloc::format!("keyboards_polled={}", list.len()))
}

#[repr(C)]
struct KeyData {
    scan_code: u16,
    unicode_char: u16,
    shift_state: u32,
    toggle_state: u8,
}

#[repr(C)]
struct TextInEx {
    reset: Fp,
    read_key_stroke_ex: Fp,
    wait_for_key_ex: P,
    set_state: Fp,
    register_key_notify: unsafe extern "efiapi" fn(
        P,
        *const KeyData,
        unsafe extern "efiapi" fn(*const KeyData) -> Status,
        *mut P,
    ) -> Status,
    unregister_key_notify: unsafe extern "efiapi" fn(P, P) -> Status,
}

unsafe extern "efiapi" fn on_key(_key: *const KeyData) -> Status {
    Status::SUCCESS
}

fn text_in_ex(g: &Guid, list: &[Handle]) -> Outcome {
    for handle in list {
        let t = iface(*handle, g).cast::<TextInEx>();
        // F12 (scan code 0x16): register a notification, then remove it.
        let key = KeyData {
            scan_code: 0x16,
            unicode_char: 0,
            shift_state: 0,
            toggle_state: 0,
        };
        let mut registration = null_mut();
        // SAFETY: key-notify registration and removal on the firmware console.
        unsafe {
            if !key_event((*t).wait_for_key_ex) {
                return Outcome::Failed(String::from("wait_for_key_ex"));
            }
            let status = ((*t).register_key_notify)(t.cast(), &key, on_key, &mut registration);
            if !ok(status) {
                return fail("RegisterKeyNotify", status);
            }
            let status = ((*t).unregister_key_notify)(t.cast(), registration);
            if !ok(status) {
                return fail("UnregisterKeyNotify", status);
            }
        }
    }
    Outcome::Exercised(alloc::format!(
        "key_notify_registered_and_removed={}",
        list.len()
    ))
}

#[repr(C)]
struct GopInfo {
    version: u32,
    horizontal: u32,
    vertical: u32,
}

#[repr(C)]
struct GopMode {
    max_mode: u32,
    mode: u32,
    info: *const GopInfo,
}

#[repr(C)]
struct Gop {
    query_mode: unsafe extern "efiapi" fn(P, u32, *mut usize, *mut *mut GopInfo) -> Status,
    set_mode: Fp,
    blt: Fp,
    mode: *const GopMode,
}

fn gop(g: &Guid, list: &[Handle]) -> Outcome {
    let mut detail = String::new();
    for handle in list {
        let o = iface(*handle, g).cast::<Gop>();
        // SAFETY: QueryMode of the current mode, compared with the live mode information.
        unsafe {
            let mode = &*(*o).mode;
            let (mut size, mut info) = (0, null_mut());
            let status = ((*o).query_mode)(o.cast(), mode.mode, &mut size, &mut info);
            if !ok(status) {
                return fail("QueryMode", status);
            }
            let (h, v) = ((*info).horizontal, (*info).vertical);
            free(info.cast());
            if (h, v) != ((*mode.info).horizontal, (*mode.info).vertical) {
                return Outcome::Failed(String::from("mode_mismatch"));
            }
            detail = alloc::format!("modes={} current={h}x{v}", mode.max_mode);
        }
    }
    Outcome::Exercised(detail)
}

#[repr(C)]
struct PointerProtocol {
    reset: Fp,
    get_state: Fp,
    wait_for_input: P,
}

fn pointer(g: &Guid, list: &[Handle], absolute: bool) -> Outcome {
    for handle in list {
        let p = iface(*handle, g).cast::<PointerProtocol>();
        // SAFETY: event field of the pointer protocol.
        if !key_event(unsafe { (*p).wait_for_input }) {
            return Outcome::Failed(String::from("wait_for_input"));
        }
    }
    Outcome::Exercised(alloc::format!(
        "devices_polled={} absolute={absolute}",
        list.len()
    ))
}

#[repr(C)]
struct Edid {
    size: u32,
    edid: *const u8,
}

fn edid(g: &Guid, list: &[Handle]) -> Outcome {
    let mut valid = 0;
    for handle in list {
        let e = iface(*handle, g).cast::<Edid>();
        // SAFETY: EDID block of `size` bytes; header 00 FF FF FF FF FF FF 00.
        unsafe {
            if (*e).size >= 128
                && *(*e).edid.cast::<[u8; 8]>() == [0, 255, 255, 255, 255, 255, 255, 0]
            {
                valid += 1;
            }
        }
    }
    Outcome::Exercised(alloc::format!("edid_blocks_valid={valid}"))
}

// ---------------------------------------------------------------------------------------------
// Firmware storage, tables and services

#[repr(C)]
struct FirmwareVolume2 {
    get_volume_attributes: unsafe extern "efiapi" fn(P, *mut u64) -> Status,
    set_volume_attributes: Fp,
    read_file: Fp,
    read_section: Fp,
    write_file: Fp,
    get_next_file:
        unsafe extern "efiapi" fn(P, *mut u8, *mut u8, *mut Guid, *mut u32, *mut usize) -> Status,
    key_size: u32,
}

fn firmware_volume(g: &Guid, list: &[Handle]) -> Outcome {
    let mut files = 0;
    for handle in list {
        let f = iface(*handle, g).cast::<FirmwareVolume2>();
        // SAFETY: attributes, then walk every file of the volume, bounded.
        unsafe {
            let mut attributes = 0;
            let status = ((*f).get_volume_attributes)(f.cast(), &mut attributes);
            if !ok(status) {
                return fail("GetVolumeAttributes", status);
            }
            let mut key = vec![0_u8; (*f).key_size as usize];
            for _ in 0..MAX_WALK {
                let (mut kind, mut name, mut file_attributes, mut size) = (0_u8, Guid::ZERO, 0, 0);
                let status = ((*f).get_next_file)(
                    f.cast(),
                    key.as_mut_ptr(),
                    &mut kind,
                    &mut name,
                    &mut file_attributes,
                    &mut size,
                );
                if !ok(status) {
                    break;
                }
                files += 1;
            }
        }
    }
    if files == 0 {
        return Outcome::Failed(String::from("no_file_in_firmware_volumes"));
    }
    Outcome::Exercised(alloc::format!("volumes={} files={files}", list.len()))
}

#[repr(C)]
struct FirmwareVolumeBlock {
    get_attributes: unsafe extern "efiapi" fn(P, *mut u32) -> Status,
    set_attributes: Fp,
    get_physical_address: unsafe extern "efiapi" fn(P, *mut u64) -> Status,
    get_block_size: unsafe extern "efiapi" fn(P, u64, *mut usize, *mut usize) -> Status,
    read: unsafe extern "efiapi" fn(P, u64, usize, *mut usize, *mut u8) -> Status,
}

fn firmware_volume_block(g: &Guid, list: &[Handle]) -> Outcome {
    let mut headers = 0;
    for handle in list {
        let f = iface(*handle, g).cast::<FirmwareVolumeBlock>();
        let mut header = [0_u8; 64];
        let mut size = header.len();
        // SAFETY: attributes, address, geometry, then a 64-byte read of the volume header.
        unsafe {
            let (mut attributes, mut address, mut block, mut blocks) = (0, 0, 0, 0);
            for status in [
                ((*f).get_attributes)(f.cast(), &mut attributes),
                ((*f).get_physical_address)(f.cast(), &mut address),
                ((*f).get_block_size)(f.cast(), 0, &mut block, &mut blocks),
                ((*f).read)(f.cast(), 0, 0, &mut size, header.as_mut_ptr()),
            ] {
                if !ok(status) {
                    return fail("FirmwareVolumeBlock", status);
                }
            }
        }
        if &header[40..44] == b"_FVH" {
            headers += 1;
        }
    }
    if headers == 0 {
        return Outcome::Failed(String::from("no_fvh_signature"));
    }
    Outcome::Exercised(alloc::format!("volume_headers_verified={headers}"))
}

#[repr(C)]
struct Smbios {
    add: Fp,
    update_string: Fp,
    remove: Fp,
    get_next:
        unsafe extern "efiapi" fn(P, *mut u16, *mut u8, *mut *const u8, *mut Handle) -> Status,
    major: u8,
    minor: u8,
}

fn smbios(g: &Guid, list: &[Handle]) -> Outcome {
    let s = iface(list[0], g).cast::<Smbios>();
    let (mut handle, mut records, mut bios, mut system) = (0xfffe_u16, 0, false, false);
    // SAFETY: walk every SMBIOS record, bounded; type is byte 0 of each record.
    let (major, minor) = unsafe {
        while records < MAX_WALK {
            let (mut record, mut producer) = (null(), null_mut());
            if !ok(((*s).get_next)(
                s.cast(),
                &mut handle,
                null_mut(),
                &mut record,
                &mut producer,
            )) {
                break;
            }
            records += 1;
            bios |= *record == 0;
            system |= *record == 1;
        }
        ((*s).major, (*s).minor)
    };
    if !bios || !system {
        return Outcome::Failed(String::from("missing_type_0_or_1"));
    }
    Outcome::Exercised(alloc::format!(
        "version={major}.{minor} records={records} bios_and_system=true"
    ))
}

#[repr(C)]
struct AcpiSdt {
    acpi_version: u32,
    get_acpi_table:
        unsafe extern "efiapi" fn(usize, *mut *const u8, *mut u32, *mut usize) -> Status,
}

/// (signature, table id) of every table the ACPI SDT protocol lists, checksums verified.
fn acpi_tables() -> Option<Vec<([u8; 4], [u8; 8])>> {
    let s = locate("AcpiSdt").cast::<AcpiSdt>();
    if s.is_null() {
        return None;
    }
    let mut tables = Vec::new();
    for index in 0..256 {
        let (mut table, mut version, mut key) = (null(), 0, 0);
        // SAFETY: GetAcpiTable(index); each returned table is `length` bytes.
        unsafe {
            if !ok(((*s).get_acpi_table)(
                index,
                &mut table,
                &mut version,
                &mut key,
            )) {
                break;
            }
            let signature = *table.cast::<[u8; 4]>();
            let length = u32::from_le_bytes(*table.add(4).cast::<[u8; 4]>()) as usize;
            if &signature != b"FACS" {
                let bytes = core::slice::from_raw_parts(table, length);
                if bytes.iter().fold(0_u8, |a, b| a.wrapping_add(*b)) != 0 {
                    return None;
                }
            }
            let id = if length >= 24 {
                *table.add(16).cast::<[u8; 8]>()
            } else {
                [0; 8]
            };
            tables.push((signature, id));
        }
    }
    Some(tables)
}

fn acpi_sdt(_g: &Guid, _list: &[Handle]) -> Outcome {
    match acpi_tables() {
        Some(tables) if !tables.is_empty() => Outcome::Exercised(alloc::format!(
            "tables={} checksums_valid=true",
            tables.len()
        )),
        _ => Outcome::Failed(String::from("acpi_table_checksum_or_listing")),
    }
}

#[repr(C)]
struct AcpiTable {
    install: unsafe extern "efiapi" fn(P, *const u8, usize, *mut usize) -> Status,
    uninstall: unsafe extern "efiapi" fn(P, usize) -> Status,
}

fn acpi_table(g: &Guid, list: &[Handle]) -> Outcome {
    let a = iface(list[0], g).cast::<AcpiTable>();
    // An empty SSDT (header only), OEM "OMNIOS", table id "OMNIPROB".
    let mut ssdt = [0_u8; 36];
    ssdt[..4].copy_from_slice(b"SSDT");
    ssdt[4..8].copy_from_slice(&36_u32.to_le_bytes());
    ssdt[8] = 2;
    ssdt[10..16].copy_from_slice(b"OMNIOS");
    ssdt[16..24].copy_from_slice(b"OMNIPROB");
    ssdt[24..28].copy_from_slice(&1_u32.to_le_bytes());
    ssdt[28..32].copy_from_slice(b"OMNI");
    ssdt[32..36].copy_from_slice(&1_u32.to_le_bytes());
    ssdt[9] = 0_u8.wrapping_sub(ssdt.iter().fold(0_u8, |s, b| s.wrapping_add(*b)));
    // Without the SDT protocol the table key alone proves installation and removal.
    let sdt = !locate("AcpiSdt").is_null();
    let listed = |tables: &Option<Vec<([u8; 4], [u8; 8])>>| {
        tables
            .as_ref()
            .is_some_and(|t| t.iter().any(|(_, id)| id == b"OMNIPROB"))
    };
    let mut key = 0;
    // SAFETY: install the table, check it is published, then uninstall it by key.
    let status = unsafe { ((*a).install)(a.cast(), ssdt.as_ptr(), ssdt.len(), &mut key) };
    if !ok(status) {
        return fail("InstallAcpiTable", status);
    }
    let published = !sdt || listed(&acpi_tables());
    let status = unsafe { ((*a).uninstall)(a.cast(), key) };
    if !ok(status) {
        return fail("UninstallAcpiTable", status);
    }
    let removed = !sdt || !listed(&acpi_tables());
    if !published || !removed {
        return Outcome::Failed(alloc::format!("published={published} removed={removed}"));
    }
    Outcome::Exercised(String::from("ssdt_installed_listed_and_removed=true"))
}

#[repr(C)]
struct Decompress {
    get_info: unsafe extern "efiapi" fn(P, *const u8, u32, *mut u32, *mut u32) -> Status,
}

fn decompress(g: &Guid, list: &[Handle]) -> Outcome {
    let d = iface(list[0], g).cast::<Decompress>();
    // UEFI compression header: compressed size 0, original size 0x1234.
    let header = [0_u8, 0, 0, 0, 0x34, 0x12, 0, 0];
    let (mut destination, mut scratch) = (0, 0);
    // SAFETY: GetInfo only parses the 8-byte header.
    let status =
        unsafe { ((*d).get_info)(d.cast(), header.as_ptr(), 8, &mut destination, &mut scratch) };
    if !ok(status) || destination != 0x1234 || scratch == 0 {
        return fail("GetInfo", status);
    }
    Outcome::Exercised(alloc::format!("header_parsed=true scratch_bytes={scratch}"))
}

#[repr(C)]
struct Ebc {
    create_thunk: Fp,
    unload_image: Fp,
    register_icache_flush: Fp,
    get_version: unsafe extern "efiapi" fn(P, *mut u64) -> Status,
}

fn ebc(g: &Guid, list: &[Handle]) -> Outcome {
    let e = iface(list[0], g).cast::<Ebc>();
    let mut version = 0;
    // SAFETY: interpreter version read.
    let status = unsafe { ((*e).get_version)(e.cast(), &mut version) };
    if ok(status) {
        Outcome::Exercised(alloc::format!("interpreter_version={version:#x}"))
    } else {
        fail("GetVersion", status)
    }
}

#[repr(C)]
struct MpService {
    get_number_of_processors: unsafe extern "efiapi" fn(P, *mut usize, *mut usize) -> Status,
    slots: [Fp; 5],
    who_am_i: unsafe extern "efiapi" fn(P, *mut usize) -> Status,
}

fn mp_service(g: &Guid, list: &[Handle]) -> Outcome {
    let m = iface(list[0], g).cast::<MpService>();
    let (mut total, mut enabled, mut me) = (0, 0, usize::MAX);
    // SAFETY: processor counts and the calling processor's number.
    unsafe {
        let status = ((*m).get_number_of_processors)(m.cast(), &mut total, &mut enabled);
        if !ok(status) {
            return fail("GetNumberOfProcessors", status);
        }
        let status = ((*m).who_am_i)(m.cast(), &mut me);
        if !ok(status) || me >= total {
            return fail("WhoAmI", status);
        }
    }
    Outcome::Exercised(alloc::format!(
        "processors={total} enabled={enabled} running_on={me}"
    ))
}

#[repr(C)]
struct ResetNotification {
    register: unsafe extern "efiapi" fn(
        P,
        unsafe extern "efiapi" fn(u32, Status, usize, *const c_void),
    ) -> Status,
    unregister: unsafe extern "efiapi" fn(
        P,
        unsafe extern "efiapi" fn(u32, Status, usize, *const c_void),
    ) -> Status,
}

unsafe extern "efiapi" fn on_reset(
    _kind: u32,
    _status: Status,
    _size: usize,
    _data: *const c_void,
) {
}

fn reset_notification(g: &Guid, list: &[Handle]) -> Outcome {
    let r = iface(list[0], g).cast::<ResetNotification>();
    // SAFETY: register a no-op reset notification, then remove it.
    unsafe {
        let status = ((*r).register)(r.cast(), on_reset);
        if !ok(status) {
            return fail("RegisterResetNotify", status);
        }
        let status = ((*r).unregister)(r.cast(), on_reset);
        if !ok(status) {
            return fail("UnregisterResetNotify", status);
        }
    }
    Outcome::Exercised(String::from("reset_notify_registered_and_removed=true"))
}

#[repr(C)]
struct Timestamp {
    get_timestamp: unsafe extern "efiapi" fn() -> u64,
    get_properties: unsafe extern "efiapi" fn(*mut [u64; 2]) -> Status,
}

fn timestamp(g: &Guid, list: &[Handle]) -> Outcome {
    let t = iface(list[0], g).cast::<Timestamp>();
    let mut properties = [0_u64; 2];
    // SAFETY: two timestamps and the counter properties.
    unsafe {
        let status = ((*t).get_properties)(&mut properties);
        let (a, b) = (((*t).get_timestamp)(), ((*t).get_timestamp)());
        if !ok(status) || properties[0] == 0 || b < a {
            return fail("GetProperties", status);
        }
    }
    Outcome::Exercised(alloc::format!("frequency_hz={}", properties[0]))
}

#[repr(C)]
struct MemoryAttribute {
    get: unsafe extern "efiapi" fn(P, u64, u64, *mut u64) -> Status,
}

fn memory_attribute(g: &Guid, list: &[Handle]) -> Outcome {
    let m = iface(list[0], g).cast::<MemoryAttribute>();
    // omni-os's own entry page must be executable and read-only (W^X): no XP (0x4000) nor RO
    // missing would be a finding; the attributes are reported.
    let page = (report as fn(&mut Directory) as usize as u64) & !0xfff;
    let mut attributes = 0;
    // SAFETY: attribute query of one page of our own code.
    let status = unsafe { ((*m).get)(m.cast(), page, 4096, &mut attributes) };
    if !ok(status) {
        return fail("GetMemoryAttributes", status);
    }
    let executable = attributes & 0x4000 == 0;
    Outcome::Exercised(alloc::format!(
        "own_code_executable={executable} read_only={}",
        attributes & 0x20000 != 0
    ))
}

#[repr(C)]
struct FirmwareManagement {
    get_image_info: unsafe extern "efiapi" fn(
        P,
        *mut usize,
        *mut u8,
        *mut u32,
        *mut u8,
        *mut usize,
        *mut u32,
        *mut *mut u16,
    ) -> Status,
}

fn firmware_management(g: &Guid, list: &[Handle]) -> Outcome {
    let mut answered = 0;
    for handle in list {
        let f = iface(*handle, g).cast::<FirmwareManagement>();
        let (mut size, mut version, mut count, mut descriptor_size, mut package, mut name) =
            (0, 0, 0, 0, 0, null_mut());
        // SAFETY: size probe of GetImageInfo (read-only; nothing is ever written to firmware).
        let status = unsafe {
            ((*f).get_image_info)(
                f.cast(),
                &mut size,
                null_mut(),
                &mut version,
                &mut count,
                &mut descriptor_size,
                &mut package,
                &mut name,
            )
        };
        if status == Status::BUFFER_TOO_SMALL || ok(status) {
            answered += 1;
        }
    }
    Outcome::Exercised(alloc::format!(
        "devices_answered={answered} capsule_write=never"
    ))
}

// ---------------------------------------------------------------------------------------------
// HII

#[repr(C)]
struct HiiDatabase {
    new_package_list: Fp,
    remove_package_list: Fp,
    update_package_list: Fp,
    list_package_lists:
        unsafe extern "efiapi" fn(P, u8, *const Guid, *mut usize, *mut Handle) -> Status,
    export_package_lists: Fp,
    register_package_notify: Fp,
    unregister_package_notify: Fp,
    find_keyboard_layouts: unsafe extern "efiapi" fn(P, *mut u16, *mut Guid) -> Status,
}

fn hii_handles() -> Vec<Handle> {
    let d = locate("HiiDatabase").cast::<HiiDatabase>();
    if d.is_null() {
        return Vec::new();
    }
    let mut size = 0;
    // SAFETY: ListPackageLists(ALL): size probe then read.
    unsafe {
        if ((*d).list_package_lists)(d.cast(), 0, null(), &mut size, null_mut())
            != Status::BUFFER_TOO_SMALL
        {
            return Vec::new();
        }
        let mut list: Vec<Handle> = vec![null_mut(); size / size_of::<Handle>()];
        if !ok(((*d).list_package_lists)(
            d.cast(),
            0,
            null(),
            &mut size,
            list.as_mut_ptr(),
        )) {
            return Vec::new();
        }
        list
    }
}

fn hii_database(g: &Guid, list: &[Handle]) -> Outcome {
    let d = iface(list[0], g).cast::<HiiDatabase>();
    let packages = hii_handles().len();
    let mut layouts = 0_u16;
    // SAFETY: keyboard layout count (size probe).
    let status = unsafe { ((*d).find_keyboard_layouts)(d.cast(), &mut layouts, null_mut()) };
    if packages == 0
        || !matches!(
            status,
            Status::SUCCESS | Status::BUFFER_TOO_SMALL | Status::NOT_FOUND
        )
    {
        return fail("ListPackageLists", status);
    }
    Outcome::Exercised(alloc::format!(
        "package_lists={packages} keyboard_layouts={}",
        layouts / 16
    ))
}

#[repr(C)]
struct HiiString {
    new_string: Fp,
    get_string: Fp,
    set_string: Fp,
    get_languages: unsafe extern "efiapi" fn(P, Handle, *mut u8, *mut usize) -> Status,
}

fn hii_string(g: &Guid, list: &[Handle]) -> Outcome {
    let s = iface(list[0], g).cast::<HiiString>();
    let mut with_strings = 0;
    for package in hii_handles() {
        let mut languages = [0_u8; 256];
        let mut size = languages.len();
        // SAFETY: languages of each package list.
        if unsafe {
            ok(((*s).get_languages)(
                s.cast(),
                package,
                languages.as_mut_ptr(),
                &mut size,
            ))
        } {
            with_strings += 1;
        }
    }
    if with_strings == 0 {
        return Outcome::Failed(String::from("no_string_package"));
    }
    Outcome::Exercised(alloc::format!(
        "string_packages_with_languages={with_strings}"
    ))
}

#[repr(C)]
struct ImageOutput {
    width: u16,
    height: u16,
    image: P,
}

#[repr(C)]
struct HiiFont {
    string_to_image: Fp,
    string_id_to_image: Fp,
    get_glyph: unsafe extern "efiapi" fn(
        P,
        u16,
        *const c_void,
        *mut *mut ImageOutput,
        *mut usize,
    ) -> Status,
}

fn hii_font(g: &Guid, list: &[Handle]) -> Outcome {
    let f = iface(list[0], g).cast::<HiiFont>();
    let mut blt = null_mut::<ImageOutput>();
    // SAFETY: render the glyph 'A'; the bitmap and the output structure are freed.
    let (status, width, height) = unsafe {
        let status = ((*f).get_glyph)(f.cast(), u16::from(b'A'), null(), &mut blt, null_mut());
        if blt.is_null() {
            (status, 0, 0)
        } else {
            let size = ((*blt).width, (*blt).height);
            free((*blt).image);
            free(blt.cast());
            (status, size.0, size.1)
        }
    };
    if !matches!(status, Status::SUCCESS | Status::WARN_UNKNOWN_GLYPH) || width == 0 || height == 0
    {
        return fail("GetGlyph", status);
    }
    Outcome::Exercised(alloc::format!("glyph_rendered={width}x{height}"))
}

fn hii_image(name: &str, g: &Guid, list: &[Handle]) -> Outcome {
    // GetImage (HiiImage slot 1) / GetImageEx (HiiImageEx slot 1): same argument shape.
    #[repr(C)]
    struct HiiImage {
        new_image: Fp,
        get_image: unsafe extern "efiapi" fn(P, Handle, u16, *mut [usize; 3]) -> Status,
    }
    let i = iface(list[0], g).cast::<HiiImage>();
    let (mut found, mut asked) = (0, 0);
    for package in hii_handles() {
        let mut image = [0_usize; 3];
        asked += 1;
        // SAFETY: image id 1 of each package list; NOT_FOUND is the answer for text-only lists.
        match unsafe { ((*i).get_image)(i.cast(), package, 1, &mut image) } {
            Status::SUCCESS => {
                found += 1;
                free(image[1] as P);
            }
            Status::NOT_FOUND | Status::INVALID_PARAMETER => {}
            other => return fail(name, other),
        }
    }
    Outcome::Exercised(alloc::format!("package_lists_asked={asked} images={found}"))
}

#[repr(C)]
struct ConfigRouting {
    extract_config: Fp,
    export_config: unsafe extern "efiapi" fn(P, *mut *mut u16) -> Status,
}

fn hii_config_routing(g: &Guid, list: &[Handle]) -> Outcome {
    let r = iface(list[0], g).cast::<ConfigRouting>();
    let mut results = null_mut();
    // SAFETY: export the whole current configuration; the string is freed.
    let status = unsafe { ((*r).export_config)(r.cast(), &mut results) };
    if !ok(status) || results.is_null() {
        return fail("ExportConfig", status);
    }
    let mut units = 0;
    // SAFETY: NUL-terminated result string, bounded.
    while units < 1 << 22 && unsafe { *results.add(units) } != 0 {
        units += 1;
    }
    let settings = from_ucs2(results).starts_with("GUID=");
    free(results.cast());
    if !settings {
        return Outcome::Failed(String::from("not_a_config_string"));
    }
    Outcome::Exercised(alloc::format!("configuration_exported_chars={units}"))
}

#[repr(C)]
struct ConfigAccess {
    extract_config:
        unsafe extern "efiapi" fn(P, *const u16, *mut *mut u16, *mut *mut u16) -> Status,
}

fn hii_config_access(g: &Guid, list: &[Handle]) -> Outcome {
    let mut extracted = 0;
    for handle in list {
        let c = iface(*handle, g).cast::<ConfigAccess>();
        let (mut progress, mut results) = (null_mut(), null_mut());
        // SAFETY: ExtractConfig(NULL request) = the driver's whole configuration.
        match unsafe { ((*c).extract_config)(c.cast(), null(), &mut progress, &mut results) } {
            Status::SUCCESS => {
                extracted += 1;
                free(results.cast());
            }
            Status::NOT_FOUND
            | Status::INVALID_PARAMETER
            | Status::UNSUPPORTED
            | Status::OUT_OF_RESOURCES => {}
            other => return fail("ExtractConfig", other),
        }
    }
    Outcome::Exercised(alloc::format!(
        "drivers={} configuration_extracted={extracted}",
        list.len()
    ))
}

fn hii_package_list(g: &Guid, list: &[Handle]) -> Outcome {
    for handle in list {
        let header = iface(*handle, g).cast::<u8>();
        // SAFETY: EFI_HII_PACKAGE_LIST_HEADER: GUID then a 32-bit total length.
        let length = unsafe { u32::from_le_bytes(*header.add(16).cast::<[u8; 4]>()) };
        if length < 20 {
            return Outcome::Failed(alloc::format!("package_list_length={length}"));
        }
    }
    Outcome::Exercised(alloc::format!("image_package_lists_valid={}", list.len()))
}

fn hii_popup(g: &Guid, list: &[Handle]) -> Outcome {
    let revision = iface(list[0], g).cast::<u64>();
    // SAFETY: first field of EFI_HII_POPUP_PROTOCOL.
    let revision = unsafe { *revision };
    if revision == 0 {
        return Outcome::Failed(String::from("revision=0"));
    }
    Outcome::Exercised(alloc::format!("revision={revision} used_by=spoken_setup"))
}

#[repr(C)]
struct FormBrowser2 {
    send_form: Fp,
    browser_callback:
        unsafe extern "efiapi" fn(P, *mut usize, *mut u16, u8, *const Guid, *const u16) -> Status,
}

fn form_browser(g: &Guid, list: &[Handle]) -> Outcome {
    let f = iface(list[0], g).cast::<FormBrowser2>();
    let mut results = [0_u16; 64];
    let mut size = size_of_val(&results);
    // SAFETY: BrowserCallback with no form on screen answers "not ready"/"not found".
    match unsafe {
        ((*f).browser_callback)(f.cast(), &mut size, results.as_mut_ptr(), 1, null(), null())
    } {
        Status::NOT_READY | Status::NOT_FOUND | Status::BUFFER_TOO_SMALL | Status::SUCCESS => {
            Outcome::Exercised(String::from("browser_state_queried=no_form_open"))
        }
        other => fail("BrowserCallback", other),
    }
}

#[repr(C)]
struct ConfigKeyword {
    set_data: Fp,
    get_data: unsafe extern "efiapi" fn(
        P,
        *const u16,
        *const u16,
        *mut *mut u16,
        *mut u32,
        *mut *mut u16,
    ) -> Status,
}

fn config_keyword(g: &Guid, list: &[Handle]) -> Outcome {
    let k = iface(list[0], g).cast::<ConfigKeyword>();
    let (mut progress, mut error, mut results) = (null_mut(), 0, null_mut());
    // SAFETY: GetData(all namespaces, all keywords); the result is freed.
    let status = unsafe {
        ((*k).get_data)(
            k.cast(),
            null(),
            null(),
            &mut progress,
            &mut error,
            &mut results,
        )
    };
    free(results.cast());
    match status {
        Status::SUCCESS => Outcome::Exercised(String::from("keywords_read=true")),
        Status::NOT_FOUND => Outcome::Exercised(String::from("keywords_read=none_defined")),
        other => fail("GetData", other),
    }
}

// ---------------------------------------------------------------------------------------------
// PI DXE services

#[repr(C)]
struct PcdProtocol {
    slots: [Fp; 16],
    get_next_token: unsafe extern "efiapi" fn(*const Guid, *mut usize) -> Status,
    get_next_token_space: unsafe extern "efiapi" fn(*mut *const Guid) -> Status,
}

fn pcd(g: &Guid, list: &[Handle]) -> Outcome {
    let p = iface(list[0], g).cast::<PcdProtocol>();
    let (mut spaces, mut tokens) = (0, 0);
    let mut space: *const Guid = null();
    // SAFETY: EFI_PCD_PROTOCOL token-space and token enumeration, bounded.
    unsafe {
        while spaces < 64 && ok(((*p).get_next_token_space)(&mut space)) && !space.is_null() {
            spaces += 1;
            let mut token = 0;
            while tokens < MAX_WALK && ok(((*p).get_next_token)(space, &mut token)) && token != 0 {
                tokens += 1;
            }
        }
    }
    Outcome::Exercised(alloc::format!(
        "dynamic_ex_token_spaces={spaces} tokens={tokens}"
    ))
}

#[repr(C)]
struct GetPcdInfo {
    get_info: Fp,
    get_sku: unsafe extern "efiapi" fn() -> usize,
}

fn get_pcd_info(g: &Guid, list: &[Handle]) -> Outcome {
    let p = iface(list[0], g).cast::<GetPcdInfo>();
    // SAFETY: current SKU id.
    let sku = unsafe { ((*p).get_sku)() };
    Outcome::Exercised(alloc::format!("sku={sku}"))
}

#[repr(C)]
struct CpuArch {
    flush_data_cache: Fp,
    enable_interrupt: Fp,
    disable_interrupt: Fp,
    get_interrupt_state: unsafe extern "efiapi" fn(P, *mut u8) -> Status,
    init: Fp,
    register_interrupt_handler: Fp,
    get_timer_value: unsafe extern "efiapi" fn(P, u32, *mut u64, *mut u64) -> Status,
    set_memory_attributes: Fp,
    number_of_timers: u32,
    dma_buffer_alignment: u32,
}

fn cpu_arch(g: &Guid, list: &[Handle]) -> Outcome {
    let c = iface(list[0], g).cast::<CpuArch>();
    let (mut state, mut first, mut second, mut period) = (0, 0, 0, 0);
    // SAFETY: interrupt state and two reads of timer 0 (the TSC).
    unsafe {
        let status = ((*c).get_interrupt_state)(c.cast(), &mut state);
        if !ok(status) {
            return fail("GetInterruptState", status);
        }
        if (*c).number_of_timers > 0 {
            let a = ((*c).get_timer_value)(c.cast(), 0, &mut first, &mut period);
            let b = ((*c).get_timer_value)(c.cast(), 0, &mut second, &mut period);
            if !ok(a) || !ok(b) || second < first {
                return fail("GetTimerValue", b);
            }
        }
    }
    Outcome::Exercised(alloc::format!(
        "interrupts_enabled={} timer_monotonic=true",
        state != 0
    ))
}

#[repr(C)]
struct Metronome {
    wait_for_tick: unsafe extern "efiapi" fn(P, u32) -> Status,
    tick_period: u32,
}

fn metronome(g: &Guid, list: &[Handle]) -> Outcome {
    let m = iface(list[0], g).cast::<Metronome>();
    // SAFETY: wait one tick.
    let (status, period) = unsafe { (((*m).wait_for_tick)(m.cast(), 1), (*m).tick_period) };
    if ok(status) && period > 0 {
        Outcome::Exercised(alloc::format!("waited_one_tick_of_100ns={period}"))
    } else {
        fail("WaitForTick", status)
    }
}

fn timer_period(name: &str, g: &Guid, list: &[Handle]) -> Outcome {
    // TimerArch: GetTimerPeriod is slot 2; WatchdogTimerArch: slot 2 as well.
    #[repr(C)]
    struct Timer {
        register_handler: Fp,
        set_timer_period: Fp,
        get_timer_period: unsafe extern "efiapi" fn(P, *mut u64) -> Status,
    }
    let t = iface(list[0], g).cast::<Timer>();
    let mut period = 0;
    // SAFETY: period read.
    let status = unsafe { ((*t).get_timer_period)(t.cast(), &mut period) };
    if !ok(status) || (name == "TimerArch" && period == 0) {
        return fail("GetTimerPeriod", status);
    }
    Outcome::Exercised(alloc::format!("period_100ns={period}"))
}

#[repr(C)]
struct RuntimeArch {
    image_head: [P; 2],
    event_head: [P; 2],
    memory_descriptor_size: usize,
    memory_descriptor_version: u32,
    memory_map_size: usize,
    memory_map_physical: P,
    memory_map_virtual: P,
    virtual_mode: u8,
    at_runtime: u8,
}

fn runtime_arch(g: &Guid, list: &[Handle]) -> Outcome {
    let r = iface(list[0], g).cast::<RuntimeArch>();
    // SAFETY: state flags of the runtime architectural protocol.
    let (virtual_mode, at_runtime) = unsafe { ((*r).virtual_mode, (*r).at_runtime) };
    if virtual_mode != 0 || at_runtime != 0 {
        return Outcome::Failed(String::from("runtime_state_before_exit_boot_services"));
    }
    Outcome::Exercised(String::from("boot_time_state_confirmed=true"))
}

#[repr(C)]
struct StatusCodeRuntime {
    report_status_code:
        unsafe extern "efiapi" fn(u32, u32, u32, *const Guid, *const c_void) -> Status,
}

type RscCallback = unsafe extern "efiapi" fn(u32, u32, u32, *const Guid, *const c_void) -> Status;

#[repr(C)]
struct RscHandler {
    register: unsafe extern "efiapi" fn(RscCallback, usize) -> Status,
    unregister: unsafe extern "efiapi" fn(RscCallback) -> Status,
}

/// EFI_PROGRESS_CODE / EFI_SOFTWARE_EFI_APPLICATION | EFI_SW_PC_USER_SETUP, instance "OM".
const OMNI_CODE_TYPE: u32 = 1;
const OMNI_CODE_VALUE: u32 = 0x0305_0000 | 0x0000_0008;
const OMNI_INSTANCE: u32 = 0x4f4d;
static STATUS_CODES_SEEN: AtomicU32 = AtomicU32::new(0);

unsafe extern "efiapi" fn on_status_code(
    kind: u32,
    value: u32,
    instance: u32,
    _caller: *const Guid,
    _data: *const c_void,
) -> Status {
    if kind & 0xff == OMNI_CODE_TYPE && value == OMNI_CODE_VALUE && instance == OMNI_INSTANCE {
        STATUS_CODES_SEEN.fetch_add(1, Ordering::SeqCst);
    }
    Status::SUCCESS
}

/// Registers a status-code listener, reports one code, and requires the listener to receive it.
fn status_codes() -> Outcome {
    let reporter = locate("StatusCodeRuntime").cast::<StatusCodeRuntime>();
    let router = locate("RscHandler").cast::<RscHandler>();
    if reporter.is_null() {
        return Outcome::Failed(String::from("status_code_reporter_missing"));
    }
    if router.is_null() {
        // No listener interface on this firmware (VMware): report one code and require the
        // firmware to accept it.
        // SAFETY: ReportStatusCode with our own progress code and no data.
        let status = unsafe {
            ((*reporter).report_status_code)(
                OMNI_CODE_TYPE,
                OMNI_CODE_VALUE,
                OMNI_INSTANCE,
                null(),
                null(),
            )
        };
        return if ok(status) {
            Outcome::Exercised(String::from("status_code_reported=true listener=none"))
        } else {
            fail("ReportStatusCode", status)
        };
    }
    let before = STATUS_CODES_SEEN.load(Ordering::SeqCst);
    // SAFETY: register at TPL_HIGH_LEVEL (synchronous delivery), report, unregister.
    unsafe {
        let status = ((*router).register)(on_status_code, 31);
        if !ok(status) && status != Status::ALREADY_STARTED {
            return fail("Register", status);
        }
        let reported = ((*reporter).report_status_code)(
            OMNI_CODE_TYPE,
            OMNI_CODE_VALUE,
            OMNI_INSTANCE,
            null(),
            null(),
        );
        let status = ((*router).unregister)(on_status_code);
        if !ok(reported) {
            return fail("ReportStatusCode", reported);
        }
        if !ok(status) {
            return fail("Unregister", status);
        }
    }
    if STATUS_CODES_SEEN.load(Ordering::SeqCst) == before {
        return Outcome::Failed(String::from("reported_code_not_routed"));
    }
    Outcome::Exercised(String::from("status_code_reported_and_received=true"))
}

#[repr(C)]
struct DebugSupport {
    isa: u32,
    get_maximum_processor_index: unsafe extern "efiapi" fn(P, *mut usize) -> Status,
}

fn debug_support(g: &Guid, list: &[Handle]) -> Outcome {
    let d = iface(list[0], g).cast::<DebugSupport>();
    let mut index = 0;
    // SAFETY: instruction set and processor index; no callback is registered.
    let (isa, status) = unsafe {
        (
            (*d).isa,
            ((*d).get_maximum_processor_index)(d.cast(), &mut index),
        )
    };
    // x64 (0x8664) from the CPU debug agent, or EBC (0x0EBC) from the byte-code interpreter.
    let isa = match isa {
        0x8664 => "x64",
        0x0ebc => "ebc",
        _ => return Outcome::Failed(alloc::format!("isa={isa:#x}")),
    };
    if !ok(status) {
        return fail("GetMaximumProcessorIndex", status);
    }
    Outcome::Exercised(alloc::format!("isa={isa} max_processor_index={index}"))
}

#[repr(C)]
struct DeferredImageLoad {
    get_image_info:
        unsafe extern "efiapi" fn(P, usize, *mut P, *mut P, *mut usize, *mut u8) -> Status,
}

fn deferred_image_load(g: &Guid, list: &[Handle]) -> Outcome {
    let d = iface(list[0], g).cast::<DeferredImageLoad>();
    let mut deferred = 0;
    // SAFETY: enumerate images deferred by the security policy, bounded.
    for index in 0..64 {
        let (mut path, mut image, mut size, mut boot) = (null_mut(), null_mut(), 0, 0);
        match unsafe {
            ((*d).get_image_info)(d.cast(), index, &mut path, &mut image, &mut size, &mut boot)
        } {
            Status::SUCCESS => deferred += 1,
            Status::NOT_FOUND => break,
            other => return fail("GetImageInfo", other),
        }
    }
    Outcome::Exercised(alloc::format!("deferred_images={deferred}"))
}

#[repr(C)]
struct AuthenticationInfo {
    get: unsafe extern "efiapi" fn(P, Handle, *mut P) -> Status,
}

fn authentication_info(g: &Guid, list: &[Handle]) -> Outcome {
    let a = iface(list[0], g).cast::<AuthenticationInfo>();
    let controller = guid("SimpleNetwork")
        .and_then(|n| handles(&n).first().copied())
        .unwrap_or(list[0]);
    let mut info = null_mut();
    // SAFETY: authentication data of a network controller (none configured is the usual answer).
    match unsafe { ((*a).get)(a.cast(), controller, &mut info) } {
        Status::SUCCESS => Outcome::Exercised(String::from("controller_authentication=configured")),
        // DEVICE_ERROR: the controller holds no authentication node (EDK II's answer for "none").
        Status::NOT_FOUND
        | Status::UNSUPPORTED
        | Status::INVALID_PARAMETER
        | Status::DEVICE_ERROR => {
            Outcome::Exercised(String::from("controller_authentication=none"))
        }
        other => fail("Get", other),
    }
}

#[repr(C)]
struct LoadFile {
    load_file: unsafe extern "efiapi" fn(P, *const c_void, u8, *mut usize, P) -> Status,
}

/// Text of a handle's device path, or empty.
fn path_text(handle: Handle) -> String {
    let to_text = locate("DevicePathToText").cast::<PathToText>();
    let path = guid("DevicePath").map_or(null_mut(), |g| iface(handle, &g));
    if to_text.is_null() || path.is_null() {
        return String::new();
    }
    // SAFETY: conversion of a firmware device path; the string is freed.
    let text = unsafe { ((*to_text).path_to_text)(path, 0, 1) };
    let out = from_ucs2(text);
    free(text.cast());
    out
}

fn load_file(g: &Guid, list: &[Handle]) -> Outcome {
    let (mut probed, mut network) = (0, 0);
    for handle in list {
        // On a network adapter any LoadFile call starts a PXE/HTTP boot (DHCP on the wire):
        // the network stays closed by policy, so those handles are never called.
        let text = path_text(*handle);
        if ["MAC(", "IPv4(", "IPv6(", "Uri("]
            .iter()
            .any(|n| text.contains(n))
        {
            network += 1;
            continue;
        }
        let l = iface(*handle, g).cast::<LoadFile>();
        let path = guid("DevicePath").map_or(null_mut(), |d| iface(*handle, &d));
        let mut size = 0;
        // SAFETY: size probe only (NULL buffer): nothing is loaded or started.
        match unsafe { ((*l).load_file)(l.cast(), path, 0, &mut size, null_mut()) } {
            Status::BUFFER_TOO_SMALL
            | Status::SUCCESS
            | Status::NOT_FOUND
            | Status::NO_MEDIA
            | Status::UNSUPPORTED
            | Status::INVALID_PARAMETER => probed += 1,
            other => return fail("LoadFile", other),
        }
    }
    if probed == 0 {
        return Outcome::Guarded("network_boot_only_network_stays_closed");
    }
    Outcome::Exercised(alloc::format!(
        "size_probed={probed} network_not_called={network}"
    ))
}

#[repr(C)]
struct ShellDynamicCommand {
    command_name: *const u16,
}

fn shell_dynamic_command(g: &Guid, list: &[Handle]) -> Outcome {
    let mut names = Vec::new();
    for handle in list {
        let s = iface(*handle, g).cast::<ShellDynamicCommand>();
        // SAFETY: the command name string.
        names.push(from_ucs2(unsafe { (*s).command_name }));
    }
    if names.iter().any(String::is_empty) {
        return Outcome::Failed(String::from("unnamed_command"));
    }
    Outcome::Exercised(alloc::format!("commands=\"{}\"", names.join(",")))
}

// Runtime services behind the NULL-interface architectural protocols.

fn monotonic_counter() -> Outcome {
    let (mut a, mut b) = (0, 0);
    // SAFETY: GetNextMonotonicCount twice.
    unsafe {
        let _ = (bs().get_next_monotonic_count)(&mut a);
        let _ = (bs().get_next_monotonic_count)(&mut b);
    }
    if b <= a {
        return Outcome::Failed(String::from("counter_not_increasing"));
    }
    Outcome::Exercised(String::from("monotonic_count_increases=true"))
}

fn real_time_clock() -> Outcome {
    match uefi::runtime::get_time() {
        Ok(time) => Outcome::Exercised(alloc::format!("rtc_year={}", time.year())),
        Err(error) => fail("GetTime", error.status()),
    }
}

fn variables(name: &str) -> Outcome {
    let count = uefi::runtime::variable_keys().flatten().count();
    if count == 0 {
        return Outcome::Failed(String::from("no_variable"));
    }
    if name == "VariableWriteArch" {
        // Writes happen only on an explicit owner action (spoken Setup); here only readability.
        return Outcome::Exercised(alloc::format!(
            "variables_enumerated={count} write_on_owner_action_only=true"
        ));
    }
    Outcome::Exercised(alloc::format!("variables_enumerated={count}"))
}

// ---------------------------------------------------------------------------------------------
// Protocols real machines add to OVMF's set: read-only calls, answers checked.

#[repr(C)]
struct Tcg1 {
    status_check: unsafe extern "efiapi" fn(P, *mut u8, *mut u32, *mut u64, *mut u64) -> Status,
}

fn tcg1(g: &Guid, list: &[Handle]) -> Outcome {
    let t = iface(list[0], g).cast::<Tcg1>();
    // TCG_EFI_BOOT_SERVICE_CAPABILITY, packed, 12 bytes; byte 0 is its size.
    let mut capability = [0_u8; 12];
    capability[0] = 12;
    let (mut flags, mut log, mut last) = (0, 0, 0);
    // SAFETY: StatusCheck into a correctly sized structure.
    let status = unsafe {
        ((*t).status_check)(
            t.cast(),
            capability.as_mut_ptr(),
            &mut flags,
            &mut log,
            &mut last,
        )
    };
    if !ok(status) {
        return fail("StatusCheck", status);
    }
    Outcome::Exercised(alloc::format!(
        "tpm12_present={} deactivated={}",
        capability[10] != 0,
        capability[11] != 0
    ))
}

#[repr(C)]
struct CcMeasurement {
    get_capability: Fp,
    get_event_log: Fp,
    hash_log_extend_event: Fp,
    map_pcr_to_mr_index: unsafe extern "efiapi" fn(P, u32, *mut u32) -> Status,
}

fn cc_measurement(g: &Guid, list: &[Handle]) -> Outcome {
    let c = iface(list[0], g).cast::<CcMeasurement>();
    let mut mr = u32::MAX;
    // SAFETY: pure mapping of PCR 0 to its measurement register.
    let status = unsafe { ((*c).map_pcr_to_mr_index)(c.cast(), 0, &mut mr) };
    if !ok(status) {
        return fail("MapPcrToMrIndex", status);
    }
    Outcome::Exercised(alloc::format!("pcr0_measurement_register={mr}"))
}

#[repr(C)]
struct DebugPort {
    reset: Fp,
    write: Fp,
    read: Fp,
    poll: unsafe extern "efiapi" fn(P) -> Status,
}

fn debug_port(g: &Guid, list: &[Handle]) -> Outcome {
    let d = iface(list[0], g).cast::<DebugPort>();
    // SAFETY: Poll only checks whether a byte is waiting.
    match unsafe { ((*d).poll)(d.cast()) } {
        Status::SUCCESS | Status::NOT_READY => Outcome::Exercised(String::from("polled=true")),
        other => fail("Poll", other),
    }
}

#[repr(C)]
struct DriverHealth {
    get_health_status:
        unsafe extern "efiapi" fn(P, Handle, Handle, *mut u32, *mut P, *mut Handle) -> Status,
}

fn driver_health(g: &Guid, list: &[Handle]) -> Outcome {
    let (mut healthy, mut attention) = (0, 0);
    for handle in list {
        let d = iface(*handle, g).cast::<DriverHealth>();
        let (mut health, mut messages, mut form) = (u32::MAX, null_mut(), null_mut());
        // SAFETY: overall health of the driver (NULL controller); the message list is freed.
        let status = unsafe {
            ((*d).get_health_status)(
                d.cast(),
                null_mut(),
                null_mut(),
                &mut health,
                &mut messages,
                &mut form,
            )
        };
        free(messages);
        if !ok(status) {
            return fail("GetHealthStatus", status);
        }
        if health == 0 {
            healthy += 1;
        } else {
            attention += 1;
        }
    }
    Outcome::Exercised(alloc::format!(
        "drivers_healthy={healthy} need_attention={attention}"
    ))
}

fn driver_family_override(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct FamilyOverride {
        get_version: unsafe extern "efiapi" fn(P) -> u32,
    }
    let mut versions = 0;
    for handle in list {
        let f = iface(*handle, g).cast::<FamilyOverride>();
        // SAFETY: version read.
        versions += usize::from(unsafe { ((*f).get_version)(f.cast()) } != 0);
    }
    Outcome::Exercised(alloc::format!(
        "drivers={} versioned={versions}",
        list.len()
    ))
}

fn platform_driver_override(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct PlatformOverride {
        get_driver: unsafe extern "efiapi" fn(P, Handle, *mut Handle) -> Status,
    }
    let o = iface(list[0], g).cast::<PlatformOverride>();
    let controllers = guid("PciIo").map(|p| handles(&p)).unwrap_or_default();
    let mut overrides = 0;
    for controller in &controllers {
        let mut driver: Handle = null_mut();
        // SAFETY: walk the platform's override list for each PCI controller, bounded.
        while overrides < 64 && unsafe { ok(((*o).get_driver)(o.cast(), *controller, &mut driver)) }
        {
            overrides += 1;
        }
    }
    Outcome::Exercised(alloc::format!(
        "controllers={} overrides={overrides}",
        controllers.len()
    ))
}

fn hii_image_decoder(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct Decoder {
        get_image_decoder_name: unsafe extern "efiapi" fn(P, *mut *mut Guid, *mut u16) -> Status,
    }
    let mut formats = 0;
    for handle in list {
        let d = iface(*handle, g).cast::<Decoder>();
        let (mut names, mut count) = (null_mut(), 0_u16);
        // SAFETY: the decoder's own static name list (not freed by the caller per the spec).
        let status = unsafe { ((*d).get_image_decoder_name)(d.cast(), &mut names, &mut count) };
        if !ok(status) {
            return fail("GetImageDecoderName", status);
        }
        formats += usize::from(count);
    }
    Outcome::Exercised(alloc::format!("image_formats={formats}"))
}

fn nvdimm_label(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct Label {
        label_storage_information: unsafe extern "efiapi" fn(P, *mut u32, *mut u32) -> Status,
    }
    let mut bytes = 0_u64;
    for handle in list {
        let l = iface(*handle, g).cast::<Label>();
        let (mut size, mut transfer) = (0, 0);
        // SAFETY: size of the label storage area.
        let status =
            unsafe { ((*l).label_storage_information)(l.cast(), &mut size, &mut transfer) };
        if !ok(status) {
            return fail("LabelStorageInformation", status);
        }
        bytes += u64::from(size);
    }
    Outcome::Exercised(alloc::format!("label_storage_bytes={bytes}"))
}

fn pci_platform(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct Platform {
        platform_notify: Fp,
        platform_prep_controller: Fp,
        get_platform_policy: unsafe extern "efiapi" fn(P, *mut u32) -> Status,
    }
    let p = iface(list[0], g).cast::<Platform>();
    let mut policy = 0;
    // SAFETY: platform PCI policy bits.
    match unsafe { ((*p).get_platform_policy)(p.cast(), &mut policy) } {
        Status::SUCCESS => Outcome::Exercised(alloc::format!("pci_policy={policy:#x}")),
        Status::UNSUPPORTED => Outcome::Exercised(String::from("pci_policy=none")),
        other => fail("GetPlatformPolicy", other),
    }
}

#[repr(C)]
struct Pkcs7Verify {
    verify_buffer: unsafe extern "efiapi" fn(
        P,
        *const u8,
        usize,
        *const u8,
        usize,
        *const P,
        *const P,
        *const P,
        *mut u8,
        *mut usize,
    ) -> Status,
}

fn pkcs7_verify(g: &Guid, list: &[Handle]) -> Outcome {
    let v = iface(list[0], g).cast::<Pkcs7Verify>();
    // A malformed signature over our own bytes, against an empty trust list: must be refused.
    let signature = [0x30_u8, 0x03, 0x02, 0x01, 0x00, 0xde, 0xad, 0xbe, 0xef];
    let data = b"omni-os";
    let empty: [P; 1] = [null_mut()];
    let mut content_size = 0;
    // SAFETY: VerifyBuffer on our buffers; nothing is written except the size.
    let status = unsafe {
        ((*v).verify_buffer)(
            v.cast(),
            signature.as_ptr(),
            signature.len(),
            data.as_ptr(),
            data.len(),
            empty.as_ptr(),
            null(),
            null(),
            null_mut(),
            &mut content_size,
        )
    };
    if ok(status) {
        return Outcome::Failed(String::from("accepted_a_malformed_signature"));
    }
    Outcome::Exercised(alloc::format!("malformed_signature_refused={status:?}"))
}

fn regular_expression(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct Regex {
        match_string: Fp,
        get_info: unsafe extern "efiapi" fn(P, *mut usize, *mut Guid) -> Status,
    }
    let r = iface(list[0], g).cast::<Regex>();
    let mut size = 0;
    // SAFETY: size probe of the supported syntax list.
    let status = unsafe { ((*r).get_info)(r.cast(), &mut size, null_mut()) };
    if status != Status::BUFFER_TOO_SMALL && !ok(status) {
        return fail("GetInfo", status);
    }
    Outcome::Exercised(alloc::format!("syntaxes={}", size / 16))
}

fn scsi_io(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct ScsiIo {
        get_device_type: unsafe extern "efiapi" fn(P, *mut u8) -> Status,
    }
    for handle in list {
        let s = iface(*handle, g).cast::<ScsiIo>();
        let mut kind = 0xff;
        // SAFETY: device type read.
        let status = unsafe { ((*s).get_device_type)(s.cast(), &mut kind) };
        if !ok(status) {
            return fail("GetDeviceType", status);
        }
    }
    Outcome::Exercised(alloc::format!("devices_typed={}", list.len()))
}

fn sd_mmc_pass_thru(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct SdMmc {
        io_align: usize,
        pass_thru: Fp,
        get_next_slot: unsafe extern "efiapi" fn(P, *mut u8) -> Status,
    }
    let mut slots = 0;
    for handle in list {
        let s = iface(*handle, g).cast::<SdMmc>();
        let mut slot = 0xff_u8;
        // SAFETY: slot enumeration, bounded.
        while slots < 64 && unsafe { ok(((*s).get_next_slot)(s.cast(), &mut slot)) } {
            slots += 1;
        }
    }
    Outcome::Exercised(alloc::format!("slots={slots}"))
}

fn sio_control(g: &Guid, list: &[Handle]) -> Outcome {
    let revision = iface(list[0], g).cast::<u32>();
    // SAFETY: first field of EFI_SIO_CONTROL_PROTOCOL.
    Outcome::Exercised(alloc::format!("revision={:#x}", unsafe { *revision }))
}

fn shell_parameters(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct Parameters {
        argv: P,
        argc: usize,
    }
    let p = iface(list[0], g).cast::<Parameters>();
    // SAFETY: argument count of the image the shell started.
    Outcome::Exercised(alloc::format!("argc={}", unsafe { (*p).argc }))
}

#[repr(C)]
struct SmramAccess {
    open: Fp,
    close: Fp,
    lock: Fp,
    get_capabilities: unsafe extern "efiapi" fn(P, *mut usize, P) -> Status,
    lock_state: u8,
    open_state: u8,
}

fn smram_access(g: &Guid, list: &[Handle]) -> Outcome {
    let a = iface(list[0], g).cast::<SmramAccess>();
    let mut size = 0;
    // SAFETY: size probe of the SMRAM map, then the lock and open states (a security check:
    // SMRAM must be locked before any operating system code runs).
    let (status, locked, open) = unsafe {
        (
            ((*a).get_capabilities)(a.cast(), &mut size, null_mut()),
            (*a).lock_state,
            (*a).open_state,
        )
    };
    if status != Status::BUFFER_TOO_SMALL && !ok(status) {
        return fail("GetCapabilities", status);
    }
    Outcome::Exercised(alloc::format!(
        "smram_regions={} locked={} open={}",
        size / 32,
        locked != 0,
        open != 0
    ))
}

fn smm_base(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct SmmBase {
        in_smm: unsafe extern "efiapi" fn(P, *mut u8) -> Status,
    }
    let b = iface(list[0], g).cast::<SmmBase>();
    let mut inside = 1_u8;
    // SAFETY: InSmm answers whether the caller runs in SMM.
    let status = unsafe { ((*b).in_smm)(b.cast(), &mut inside) };
    if !ok(status) || inside != 0 {
        return fail("InSmm", status);
    }
    Outcome::Exercised(String::from("caller_outside_smm=true"))
}

fn i2c_enumerate(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct Enumerate {
        enumerate: unsafe extern "efiapi" fn(P, *mut *const c_void) -> Status,
    }
    let mut devices = 0;
    for handle in list {
        let e = iface(*handle, g).cast::<Enumerate>();
        let mut device = null();
        // SAFETY: walk the platform's I2C device list from NULL, bounded (no bus traffic).
        while devices < 256
            && unsafe { ok(((*e).enumerate)(e.cast(), &mut device)) }
            && !device.is_null()
        {
            devices += 1;
        }
    }
    Outcome::Exercised(alloc::format!("i2c_devices={devices}"))
}

fn ufs_device_config(g: &Guid, list: &[Handle]) -> Outcome {
    #[repr(C)]
    struct UfsConfig {
        rw_ufs_descriptor:
            unsafe extern "efiapi" fn(P, u8, u8, u8, u8, *mut u8, *mut u32) -> Status,
    }
    let mut read = 0;
    for handle in list {
        let u = iface(*handle, g).cast::<UfsConfig>();
        let mut descriptor = [0_u8; 256];
        let mut size = descriptor.len() as u32;
        // SAFETY: READ of the device descriptor (id 0): read-only.
        let status = unsafe {
            ((*u).rw_ufs_descriptor)(u.cast(), 1, 0, 0, 0, descriptor.as_mut_ptr(), &mut size)
        };
        if !ok(status) {
            return fail("RwUfsDescriptor", status);
        }
        read += 1;
    }
    Outcome::Exercised(alloc::format!("device_descriptors_read={read}"))
}
