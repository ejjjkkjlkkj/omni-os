//! Pre-installation environment: install omni-os from the installation medium to an internal
//! disk, spoken and keyboard-only, before any kernel runs (the role WinPE plays for Windows).
//!
//! It starts only on a medium that carries `\OMNI\INSTMED`. The internal disks are found through
//! `BlockIo` (never the medium itself), announced with their kind and size, and chosen with the
//! arrow keys; the chosen disk is named again with a spoken warning and needs a second Enter.
//! Then, with the disk opened exclusively:
//!
//! 1. a GUID Partition Table with one 512 MiB EFI system partition and a FAT32 file system on it
//!    are written (`aw-install`), flushed, and the primary header is read back and checked;
//! 2. the disk is reconnected so the firmware's partition and FAT drivers mount the new ESP;
//! 3. the loader (`\EFI\omni-os\BOOTX64.EFI` and the removable-media path), the kernel and a
//!    boot-state record making it known-good generation 1 are written, then read back and
//!    compared by SHA-256;
//! 4. a `Boot####` load option "omni-os" (short-form hard-drive path) is written and put first
//!    in `BootOrder` (UEFI 2.11, 3.1), then read back.
//!
//! Timeout or Escape never installs: the medium then boots omni-os as usual.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

use aw_bootstate::{BootSelectionState, BootStateRecord, GenerationLocator};
use aw_generation::ObjectId;
use aw_install::load_option_description;
use aw_install::{DiskPlan, Fill, fat32_writes, gpt_header_valid, gpt_writes, load_option};
use aw_sha256::sha256;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::text::{AllowShortcuts, DisplayOnly};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::block::BlockIO;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileMode};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::rng::Rng;
use uefi::runtime::{self, VariableAttributes, VariableVendor};
use uefi::{CString16, Handle, boot};

use crate::audio;
use crate::aw_mark;
use crate::recovery::read_file;
use crate::setup::{Lang, read_key_raw, speak_dynamic};

const MARKER: &str = "OMNI\\INSTMED";
const LOADER_SOURCE: &str = "EFI\\BOOT\\BOOTX64.EFI";
const LOADER_TARGET: &str = "EFI\\omni-os\\BOOTX64.EFI";
const LOADER_FALLBACK: &str = "EFI\\BOOT\\BOOTX64.EFI";
const KERNEL: &str = "KERNEL.BIN";
const STATE: &str = "OMNI\\BOOTST.A";
const DESCRIPTION: &str = "omni-os";
/// Zero-fill chunk (blocks).
const CHUNK_BLOCKS: u64 = 256;

struct Voice {
    speaker: Option<audio::Speaker>,
    pending: Option<Key>,
}

impl Voice {
    fn say(&mut self, text: &str) {
        aw_mark!("AW_INSTALL_SPEAK \"{text}\"");
        uefi::println!("{text}");
        speak_dynamic(text, Lang::Fr, &mut self.speaker, &mut self.pending);
    }
}

struct Disk {
    handle: Handle,
    path: Vec<u8>,
    text: String,
    blocks: u64,
    block_size: u32,
    plan: Option<DiskPlan>,
}

impl Disk {
    fn spoken(&self) -> String {
        let kind = if self.text.contains("NVMe") {
            "disque NVMe"
        } else if self.text.contains("Sata") {
            "disque SATA"
        } else if self.text.contains("USB") {
            "disque USB"
        } else {
            "disque"
        };
        let gib = self.blocks * u64::from(self.block_size) / (1024 * 1024 * 1024);
        format!("{kind}, {gib} gigaoctets")
    }
}

/// Device path bytes without the end node.
fn path_bytes(handle: Handle) -> Option<(Vec<u8>, String)> {
    // SAFETY: shared GetProtocol open of the device path, read only.
    let path = unsafe {
        boot::open_protocol::<DevicePath>(
            boot::OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()?;
    let bytes = path.as_bytes();
    let text = path
        .to_string16(DisplayOnly(true), AllowShortcuts(true))
        .map_or_else(|_| String::new(), |t| format!("{t}"));
    Some((bytes[..bytes.len().saturating_sub(4)].to_vec(), text))
}

/// Whole, writable disks other than the medium omni-os started from.
fn target_disks() -> Vec<Disk> {
    // The firmware may have connected only the devices of its boot order: connect every
    // controller so every internal disk has its BlockIo.
    if let Ok(all) = boot::locate_handle_buffer(boot::SearchType::AllHandles) {
        for handle in all.iter() {
            let _ = boot::connect_controller(*handle, &[], None, true);
        }
    }
    let own = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .ok()
        .and_then(|image| image.device())
        .and_then(path_bytes)
        .map(|(bytes, _)| bytes)
        .unwrap_or_default();
    let mut disks = Vec::new();
    for handle in boot::find_handles::<BlockIO>().unwrap_or_default() {
        // SAFETY: shared GetProtocol open, only the media descriptor is read.
        let Ok(block) = (unsafe {
            boot::open_protocol::<BlockIO>(
                boot::OpenProtocolParams {
                    handle,
                    agent: boot::image_handle(),
                    controller: None,
                },
                boot::OpenProtocolAttributes::GetProtocol,
            )
        }) else {
            continue;
        };
        let media = block.media();
        if media.is_logical_partition() || !media.is_media_present() || media.is_read_only() {
            continue;
        }
        let Some((path, text)) = path_bytes(handle) else {
            continue;
        };
        // The installation medium itself (its partitions share its path prefix) is never a target.
        if !path.is_empty() && own.starts_with(&path) {
            continue;
        }
        let blocks = media.last_block() + 1;
        let block_size = media.block_size();
        disks.push(Disk {
            handle,
            path,
            text,
            blocks,
            block_size,
            plan: DiskPlan::new(block_size, blocks).ok(),
        });
    }
    disks
}

enum Command {
    Previous,
    Next,
    Enter,
    Escape,
    Timeout,
}

fn next_command(voice: &mut Voice) -> Command {
    aw_mark!("AW_INSTALL_AWAITING_INPUT");
    for _ in 0..600 {
        if let Some(key) = voice.pending.take().or_else(read_key_raw) {
            aw_mark!("AW_INSTALL_KEY key={key:?}");
            return match key {
                Key::Special(ScanCode::UP) => Command::Previous,
                Key::Special(ScanCode::DOWN) => Command::Next,
                Key::Special(ScanCode::ESCAPE) => Command::Escape,
                Key::Printable(c) if char::from(c) == '\r' => Command::Enter,
                _ => continue,
            };
        }
        boot::stall(Duration::from_millis(100));
    }
    Command::Timeout
}

/// Offer the installation when omni-os runs from an installation medium.
pub fn offer(root: &mut Directory) {
    if read_file(root, MARKER).is_none() {
        return;
    }
    let mut voice = Voice {
        speaker: audio::bring_up(),
        pending: None,
    };
    let disks: Vec<Disk> = target_disks();
    let usable: Vec<&Disk> = disks.iter().filter(|d| d.plan.is_some()).collect();
    aw_mark!(
        "AW_INSTALL_ENV disks={} usable={}",
        disks.len(),
        usable.len()
    );
    voice.say("Programme d'installation d'omni-os.");
    if usable.is_empty() {
        voice.say(
            "Aucun disque interne d'au moins un gigaoctet n'est disponible. Démarrage d'omni-os.",
        );
        return;
    }
    voice.say(
        "Flèches pour choisir le disque, Entrée pour valider, Échap pour démarrer sans installer.",
    );
    // Items: each usable disk, then "boot without installing".
    let count = usable.len() + 1;
    let mut focus = 0_usize;
    let label = |index: usize| -> String {
        usable.get(index).map_or_else(
            || String::from("Démarrer omni-os sans installer"),
            |disk| {
                format!(
                    "Installer sur le {}, {} sur {}",
                    disk.spoken(),
                    index + 1,
                    usable.len()
                )
            },
        )
    };
    voice.say(&label(focus));
    let mut armed: Option<usize> = None;
    loop {
        match next_command(&mut voice) {
            Command::Next => {
                focus = (focus + 1) % count;
                armed = None;
                aw_mark!("AW_INSTALL_FOCUS index={focus}");
                voice.say(&label(focus));
            }
            Command::Previous => {
                focus = (focus + count - 1) % count;
                armed = None;
                aw_mark!("AW_INSTALL_FOCUS index={focus}");
                voice.say(&label(focus));
            }
            Command::Escape => {
                if armed.take().is_some() {
                    voice.say("Annulé.");
                } else {
                    aw_mark!("AW_INSTALL_SKIPPED reason=escape");
                    voice.say("Démarrage d'omni-os sans installer.");
                    return;
                }
            }
            Command::Timeout => {
                aw_mark!("AW_INSTALL_SKIPPED reason=timeout");
                return;
            }
            Command::Enter => {
                let Some(disk) = usable.get(focus) else {
                    aw_mark!("AW_INSTALL_SKIPPED reason=chosen");
                    voice.say("Démarrage d'omni-os sans installer.");
                    return;
                };
                if armed != Some(focus) {
                    armed = Some(focus);
                    aw_mark!("AW_INSTALL_CONFIRM_REQUIRED disk=\"{}\"", disk.text);
                    voice.say(&format!(
                        "Attention : toutes les données du {} seront effacées. Chemin : {}. Appuyez de nouveau sur Entrée pour installer, ou Échap pour annuler.",
                        disk.spoken(),
                        disk.text
                    ));
                    continue;
                }
                voice.say("Installation en cours. Ne pas éteindre l'ordinateur.");
                match install(root, disk) {
                    Ok(boot_option) => {
                        aw_mark!(
                            "AW_INSTALL_DONE disk=\"{}\" boot_option={boot_option}",
                            disk.text
                        );
                        voice.say("Installation terminée et vérifiée. Retirez le support d'installation et redémarrez : omni-os démarrera depuis ce disque.");
                    }
                    Err(reason) => {
                        aw_mark!("AW_INSTALL_FAIL reason={reason}");
                        voice.say(&format!("L'installation a échoué : {reason}."));
                    }
                }
                return;
            }
        }
    }
}

/// Page-aligned scratch buffer (satisfies any `IoAlign`), freed on drop.
struct Pages {
    ptr: core::ptr::NonNull<u8>,
    pages: usize,
}

impl Pages {
    fn new(bytes: usize) -> Option<Self> {
        let pages = bytes.div_ceil(4096).max(1);
        let ptr = boot::allocate_pages(
            boot::AllocateType::AnyPages,
            boot::MemoryType::LOADER_DATA,
            pages,
        )
        .ok()?;
        Some(Self { ptr, pages })
    }

    fn slice(&mut self, len: usize) -> &mut [u8] {
        // SAFETY: `len` never exceeds the `pages * 4096` bytes allocated in `new`.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), len) }
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        // SAFETY: pages allocated in `new`.
        let _ = unsafe { boot::free_pages(self.ptr, self.pages) };
    }
}

fn random_guid() -> [u8; 16] {
    let mut guid = [0_u8; 16];
    let random = boot::get_handle_for_protocol::<Rng>()
        .ok()
        .and_then(|h| boot::open_protocol_exclusive::<Rng>(h).ok())
        .is_some_and(|mut rng| rng.get_rng(None, &mut guid).is_ok());
    if !random {
        // No hardware RNG: derive from the clock, the time-stamp counter and a call counter
        // (unique, not secret).
        static CALLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        let call = CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // SAFETY: RDTSC has no side effects.
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        let seed = format!("{:?}{tsc}{call}", runtime::get_time());
        guid.copy_from_slice(&sha256(seed.as_bytes())[..16]);
    }
    // RFC 4122 version 4, variant 1 (EFI_GUID memory layout: version in byte 7).
    guid[7] = (guid[7] & 0x0f) | 0x40;
    guid[8] = (guid[8] & 0x3f) | 0x80;
    guid
}

fn write_disk(
    disk: &Disk,
    plan: DiskPlan,
    writes: &[aw_install::Write],
) -> Result<(), &'static str> {
    let mut block =
        boot::open_protocol_exclusive::<BlockIO>(disk.handle).map_err(|_| "disk_busy")?;
    let media_id = block.media().media_id();
    let bs = plan.block_size() as usize;
    let mut buffer = Pages::new(CHUNK_BLOCKS as usize * bs).ok_or("no_memory")?;
    for write in writes {
        match &write.fill {
            Fill::Bytes(bytes) => {
                let target = buffer.slice(bytes.len());
                target.copy_from_slice(bytes);
                block
                    .write_blocks(media_id, write.lba, target)
                    .map_err(|_| "write_failed")?;
            }
            Fill::Zero => {
                let mut lba = write.lba;
                let end = write.lba + write.blocks;
                while lba < end {
                    let n = (end - lba).min(CHUNK_BLOCKS) as usize;
                    let zeros = buffer.slice(n * bs);
                    zeros.fill(0);
                    block
                        .write_blocks(media_id, lba, zeros)
                        .map_err(|_| "write_failed")?;
                    lba += n as u64;
                }
            }
        }
    }
    block.flush_blocks().map_err(|_| "flush_failed")?;
    let header = buffer.slice(bs);
    block
        .read_blocks(media_id, 1, header)
        .map_err(|_| "read_back_failed")?;
    if !gpt_header_valid(plan, header) {
        return Err("gpt_read_back_mismatch");
    }
    Ok(())
}

/// The mounted EFI system partition (partition 1) of `disk`.
fn open_esp(disk: &Disk) -> Result<Directory, &'static str> {
    for handle in boot::find_handles::<SimpleFileSystem>().unwrap_or_default() {
        let Some((path, _)) = path_bytes(handle) else {
            continue;
        };
        // Disk path, then a hard-drive node (type 4, subtype 1) for partition number 1.
        let Some(node) = path.get(disk.path.len()..) else {
            continue;
        };
        if !path.starts_with(&disk.path) || node.len() < 8 || node[0] != 4 || node[1] != 1 {
            continue;
        }
        if u32::from_le_bytes([node[4], node[5], node[6], node[7]]) != 1 {
            continue;
        }
        let mut fs =
            boot::open_protocol_exclusive::<SimpleFileSystem>(handle).map_err(|_| "esp_busy")?;
        return fs.open_volume().map_err(|_| "esp_not_mountable");
    }
    Err("esp_not_mounted")
}

fn write_path(root: &mut Directory, path: &str, data: &[u8]) -> bool {
    // Create each parent directory, then the file itself.
    let parts: Vec<&str> = path.split('\\').collect();
    for depth in 1..parts.len() {
        let dir = parts[..depth].join("\\");
        if let Ok(name) = CString16::try_from(dir.as_str()) {
            let _ = root.open(&name, FileMode::CreateReadWrite, FileAttribute::DIRECTORY);
        }
    }
    let Ok(name) = CString16::try_from(path) else {
        return false;
    };
    let Ok(handle) = root.open(&name, FileMode::CreateReadWrite, FileAttribute::empty()) else {
        return false;
    };
    let Some(mut file) = handle.into_regular_file() else {
        return false;
    };
    file.write(data).is_ok() && file.flush().is_ok()
}

fn register_boot_option(option: &[u8]) -> Result<String, &'static str> {
    let global = VariableVendor::GLOBAL_VARIABLE;
    let attributes = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;
    // Reuse an existing "omni-os" option, else the lowest free number.
    let mut used = [false; 0x1_0000];
    let mut ours = None;
    for key in runtime::variable_keys().flatten() {
        if key.vendor != global {
            continue;
        }
        let name = format!("{}", key.name);
        let Some(hex) = name.strip_prefix("Boot").filter(|h| h.len() == 4) else {
            continue;
        };
        let Ok(number) = u16::from_str_radix(hex, 16) else {
            continue;
        };
        used[usize::from(number)] = true;
        if ours.is_none()
            && runtime::get_variable_boxed(&key.name, &global)
                .ok()
                .and_then(|(data, _)| load_option_description(&data))
                .is_some_and(|d| d == DESCRIPTION)
        {
            ours = Some(number);
        }
    }
    let number = ours
        .or_else(|| (0..=0xffff_u16).find(|n| !used[usize::from(*n)]))
        .ok_or("no_free_boot_option")?;
    let name = format!("Boot{number:04X}");
    let name16 = CString16::try_from(name.as_str()).map_err(|_| "name")?;
    runtime::set_variable(&name16, &global, attributes, option).map_err(|_| "boot_option_write")?;
    let (back, _) =
        runtime::get_variable_boxed(&name16, &global).map_err(|_| "boot_option_read")?;
    if &*back != option {
        return Err("boot_option_read_back_mismatch");
    }
    let order_name = CString16::try_from("BootOrder").map_err(|_| "name")?;
    let mut order: Vec<u16> = runtime::get_variable_boxed(&order_name, &global)
        .map(|(data, _)| {
            data.as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect()
        })
        .unwrap_or_default();
    order.retain(|n| *n != number);
    order.insert(0, number);
    let bytes: Vec<u8> = order.iter().flat_map(|n| n.to_le_bytes()).collect();
    runtime::set_variable(&order_name, &global, attributes, &bytes)
        .map_err(|_| "boot_order_write")?;
    Ok(name)
}

fn install(media: &mut Directory, disk: &Disk) -> Result<String, &'static str> {
    let plan = disk.plan.ok_or("disk_too_small")?;
    let loader = read_file(media, LOADER_SOURCE).ok_or("loader_missing_on_medium")?;
    let kernel = read_file(media, KERNEL).ok_or("kernel_missing_on_medium")?;
    let partition_guid = random_guid();
    let mut writes = gpt_writes(plan, random_guid(), partition_guid);
    let volume_id = u32::from_le_bytes([
        partition_guid[0],
        partition_guid[1],
        partition_guid[2],
        partition_guid[3],
    ]);
    writes.extend(fat32_writes(plan, volume_id).map_err(|_| "file_system_geometry")?);
    write_disk(disk, plan, &writes)?;
    aw_mark!(
        "AW_INSTALL_DISK_WRITTEN esp_first={} esp_blocks={} writes={}",
        plan.esp_first(),
        plan.esp_blocks(),
        writes.len()
    );

    // Let the firmware's partition and FAT drivers mount the new ESP.
    let _ = boot::disconnect_controller(disk.handle, None, None);
    boot::connect_controller(disk.handle, &[], None, true).map_err(|_| "reconnect_failed")?;
    let mut esp = open_esp(disk)?;

    let digest = sha256(&kernel);
    let locator = ObjectId::new(digest)
        .and_then(|id| GenerationLocator::new(1, id))
        .ok_or("kernel_digest")?;
    let state = BootStateRecord::new(1, locator, locator, 1, BootSelectionState::Successful)
        .map_err(|_| "boot_state")?
        .encode();
    let files: [(&str, &[u8]); 4] = [
        (LOADER_TARGET, &loader),
        (LOADER_FALLBACK, &loader),
        (KERNEL, &kernel),
        (STATE, &state),
    ];
    for (path, data) in files {
        if !write_path(&mut esp, path, data) {
            return Err("file_write_failed");
        }
    }
    for (path, data) in files {
        let back = read_file(&mut esp, path).ok_or("file_read_back_failed")?;
        if sha256(&back) != sha256(data) {
            return Err("file_read_back_mismatch");
        }
        aw_mark!("AW_INSTALL_FILE_VERIFIED path={path} bytes={}", back.len());
    }
    let option = load_option(
        DESCRIPTION,
        plan,
        partition_guid,
        &format!("\\{LOADER_TARGET}"),
    );
    register_boot_option(&option)
}

/// Name and description of the boot option the firmware started (`BootCurrent`), announced for
/// the owner and the proofs.
pub fn report_boot_current() {
    let global = VariableVendor::GLOBAL_VARIABLE;
    let Some(current) = CString16::try_from("BootCurrent")
        .ok()
        .and_then(|name| runtime::get_variable_boxed(&name, &global).ok())
        .and_then(|(data, _)| (data.len() == 2).then(|| u16::from_le_bytes([data[0], data[1]])))
    else {
        aw_mark!("AW_UEFI_BOOT_CURRENT present=false");
        return;
    };
    let name = format!("Boot{current:04X}");
    let description = CString16::try_from(name.as_str())
        .ok()
        .and_then(|n| runtime::get_variable_boxed(&n, &global).ok())
        .and_then(|(data, _)| load_option_description(&data))
        .unwrap_or_default();
    aw_mark!("AW_UEFI_BOOT_CURRENT option={name} description=\"{description}\"");
}
