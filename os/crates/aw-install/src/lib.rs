//! Pre-installation environment, pure part: the exact bytes that make a blank disk an omni-os
//! system disk, computed without any I/O so they are tested on the host.
//!
//! - a GUID Partition Table (UEFI 2.11 chapter 5): protective MBR, primary and backup headers
//!   and partition arrays with their CRC-32, one EFI system partition of [`ESP_BYTES`],
//!   1 MiB-aligned; the rest of the disk stays unallocated for the system store;
//! - a FAT32 file system on that partition (the format UEFI 2.11, 13.3 requires for an ESP);
//! - the `Boot####` load option (UEFI 2.11, 3.1.3) whose device path is the short-form hard
//!   drive node of that partition followed by the loader's file path.
//!
//! The loader performs the writes ([`Write`]) with `BlockIo`, reconnects the disk, copies the
//! files through the firmware's FAT driver and reads everything back.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use aw_bootstate::crc32;

/// Size of the EFI system partition written by the installer.
pub const ESP_BYTES: u64 = 512 * 1024 * 1024;
/// `C12A7328-F81F-11D2-BA4B-00A0C93EC93B` in EFI_GUID memory layout.
pub const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];
const GPT_ENTRIES: u64 = 128;
const GPT_ENTRY_BYTES: u64 = 128;
const GPT_HEADER_BYTES: u32 = 92;
const ALIGNMENT_BYTES: u64 = 1024 * 1024;
const FAT32_RESERVED_SECTORS: u64 = 32;
const FAT32_MIN_CLUSTERS: u64 = 65_525;
const FAT32_MAX_CLUSTERS: u64 = 0x0FFF_FFF5;
const CLUSTER_BYTES: u64 = 4096;
const ESP_NAME: &str = "EFI system partition";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanError {
    UnsupportedBlockSize,
    DiskTooSmall,
    FileSystemGeometry,
}

/// Where everything goes on one disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskPlan {
    block_size: u64,
    blocks: u64,
    entry_blocks: u64,
    esp_first: u64,
    esp_last: u64,
}

impl DiskPlan {
    pub fn new(block_size: u32, blocks: u64) -> Result<Self, PlanError> {
        let block_size = u64::from(block_size);
        if block_size != 512 && block_size != 4096 {
            return Err(PlanError::UnsupportedBlockSize);
        }
        let entry_blocks = (GPT_ENTRIES * GPT_ENTRY_BYTES).div_ceil(block_size);
        let esp_first = ALIGNMENT_BYTES / block_size;
        let esp_last = esp_first + ESP_BYTES / block_size - 1;
        // Room for the backup partition array and header after the partition.
        let last_usable = blocks
            .checked_sub(2 + entry_blocks)
            .ok_or(PlanError::DiskTooSmall)?;
        if esp_last > last_usable {
            return Err(PlanError::DiskTooSmall);
        }
        Ok(Self {
            block_size,
            blocks,
            entry_blocks,
            esp_first,
            esp_last,
        })
    }

    #[must_use]
    pub const fn block_size(self) -> u64 {
        self.block_size
    }

    #[must_use]
    pub const fn esp_first(self) -> u64 {
        self.esp_first
    }

    #[must_use]
    pub const fn esp_blocks(self) -> u64 {
        self.esp_last - self.esp_first + 1
    }

    const fn last_usable(self) -> u64 {
        self.blocks - 2 - self.entry_blocks
    }

    const fn backup_header(self) -> u64 {
        self.blocks - 1
    }

    const fn backup_entries(self) -> u64 {
        self.blocks - 1 - self.entry_blocks
    }
}

/// What to put in `blocks` blocks starting at `lba`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fill {
    Zero,
    /// Exactly `blocks * block_size` bytes.
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Write {
    pub lba: u64,
    pub blocks: u64,
    pub fill: Fill,
}

fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn block(plan: DiskPlan) -> Vec<u8> {
    vec![0_u8; plan.block_size as usize]
}

fn gpt_header(plan: DiskPlan, disk_guid: [u8; 16], entries_crc: u32, backup: bool) -> Vec<u8> {
    let mut out = block(plan);
    let (mine, alternate, entries) = if backup {
        (plan.backup_header(), 1, plan.backup_entries())
    } else {
        (1, plan.backup_header(), 2)
    };
    out[..8].copy_from_slice(b"EFI PART");
    put32(&mut out, 8, 0x0001_0000);
    put32(&mut out, 12, GPT_HEADER_BYTES);
    put64(&mut out, 24, mine);
    put64(&mut out, 32, alternate);
    put64(&mut out, 40, 2 + plan.entry_blocks);
    put64(&mut out, 48, plan.last_usable());
    out[56..72].copy_from_slice(&disk_guid);
    put64(&mut out, 72, entries);
    put32(&mut out, 80, GPT_ENTRIES as u32);
    put32(&mut out, 84, GPT_ENTRY_BYTES as u32);
    put32(&mut out, 88, entries_crc);
    let crc = crc32(&out[..GPT_HEADER_BYTES as usize]);
    put32(&mut out, 16, crc);
    out
}

/// Protective MBR, primary and backup GPT with one EFI system partition.
#[must_use]
pub fn gpt_writes(plan: DiskPlan, disk_guid: [u8; 16], partition_guid: [u8; 16]) -> Vec<Write> {
    let mut mbr = block(plan);
    let entry = &mut mbr[446..462];
    entry[1..4].copy_from_slice(&[0x00, 0x02, 0x00]);
    entry[4] = 0xee;
    entry[5..8].copy_from_slice(&[0xff, 0xff, 0xff]);
    put32(entry, 8, 1);
    put32(
        entry,
        12,
        u32::try_from(plan.blocks - 1).unwrap_or(u32::MAX),
    );
    mbr[510] = 0x55;
    mbr[511] = 0xaa;

    let mut entries = vec![0_u8; (plan.entry_blocks * plan.block_size) as usize];
    entries[..16].copy_from_slice(&ESP_TYPE_GUID);
    entries[16..32].copy_from_slice(&partition_guid);
    put64(&mut entries, 32, plan.esp_first);
    put64(&mut entries, 40, plan.esp_last);
    for (index, unit) in ESP_NAME.encode_utf16().enumerate() {
        put16(&mut entries, 56 + 2 * index, unit);
    }
    let entries_crc = crc32(&entries[..(GPT_ENTRIES * GPT_ENTRY_BYTES) as usize]);

    vec![
        Write {
            lba: 0,
            blocks: 1,
            fill: Fill::Bytes(mbr),
        },
        Write {
            lba: 1,
            blocks: 1,
            fill: Fill::Bytes(gpt_header(plan, disk_guid, entries_crc, false)),
        },
        Write {
            lba: 2,
            blocks: plan.entry_blocks,
            fill: Fill::Bytes(entries.clone()),
        },
        Write {
            lba: plan.backup_entries(),
            blocks: plan.entry_blocks,
            fill: Fill::Bytes(entries),
        },
        Write {
            lba: plan.backup_header(),
            blocks: 1,
            fill: Fill::Bytes(gpt_header(plan, disk_guid, entries_crc, true)),
        },
    ]
}

/// Checks a primary GPT header read back from LBA 1 against the plan (signature, header CRC,
/// location of the backup and of the partition array).
#[must_use]
pub fn gpt_header_valid(plan: DiskPlan, header: &[u8]) -> bool {
    if header.len() < GPT_HEADER_BYTES as usize || &header[..8] != b"EFI PART" {
        return false;
    }
    let mut copy = header[..GPT_HEADER_BYTES as usize].to_vec();
    let stored = u32::from_le_bytes([copy[16], copy[17], copy[18], copy[19]]);
    copy[16..20].fill(0);
    let word = |at: usize| {
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&header[at..at + 8]);
        u64::from_le_bytes(bytes)
    };
    crc32(&copy) == stored && word(24) == 1 && word(32) == plan.backup_header() && word(72) == 2
}

/// FAT32 geometry of the EFI system partition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fat32 {
    pub sectors_per_cluster: u64,
    pub fat_sectors: u64,
    pub clusters: u64,
}

pub fn fat32_geometry(plan: DiskPlan) -> Result<Fat32, PlanError> {
    let sectors_per_cluster = (CLUSTER_BYTES / plan.block_size).max(1);
    let total = plan.esp_blocks();
    let mut fat_sectors = 1;
    loop {
        let data = total
            .checked_sub(FAT32_RESERVED_SECTORS + 2 * fat_sectors)
            .ok_or(PlanError::FileSystemGeometry)?;
        let clusters = data / sectors_per_cluster;
        let needed = ((clusters + 2) * 4).div_ceil(plan.block_size);
        if needed <= fat_sectors {
            if !(FAT32_MIN_CLUSTERS..=FAT32_MAX_CLUSTERS).contains(&clusters) {
                return Err(PlanError::FileSystemGeometry);
            }
            return Ok(Fat32 {
                sectors_per_cluster,
                fat_sectors,
                clusters,
            });
        }
        fat_sectors = needed;
    }
}

fn boot_sector(plan: DiskPlan, fat: Fat32, volume_id: u32) -> Vec<u8> {
    let mut out = block(plan);
    out[..3].copy_from_slice(&[0xeb, 0x58, 0x90]);
    out[3..11].copy_from_slice(b"OMNI-OS ");
    put16(&mut out, 11, plan.block_size as u16);
    out[13] = fat.sectors_per_cluster as u8;
    put16(&mut out, 14, FAT32_RESERVED_SECTORS as u16);
    out[16] = 2;
    out[21] = 0xf8;
    put16(&mut out, 24, 63);
    put16(&mut out, 26, 255);
    put32(&mut out, 28, u32::try_from(plan.esp_first).unwrap_or(0));
    put32(&mut out, 32, plan.esp_blocks() as u32);
    put32(&mut out, 36, fat.fat_sectors as u32);
    put32(&mut out, 44, 2);
    put16(&mut out, 48, 1);
    put16(&mut out, 50, 6);
    out[64] = 0x80;
    out[66] = 0x29;
    put32(&mut out, 67, volume_id);
    out[71..82].copy_from_slice(b"OMNI-OS    ");
    out[82..90].copy_from_slice(b"FAT32   ");
    // No legacy boot code: halt forever if a BIOS ever jumps here.
    out[90..93].copy_from_slice(&[0xf4, 0xeb, 0xfd]);
    out[510] = 0x55;
    out[511] = 0xaa;
    out
}

fn fs_info(plan: DiskPlan) -> Vec<u8> {
    let mut out = block(plan);
    put32(&mut out, 0, 0x4161_5252);
    put32(&mut out, 484, 0x6141_7272);
    put32(&mut out, 488, 0xffff_ffff);
    put32(&mut out, 492, 0xffff_ffff);
    put32(&mut out, 508, 0xaa55_0000);
    out
}

/// A fresh FAT32 file system on the EFI system partition: every metadata block is written
/// (old data on a reused disk never leaks into the new file system), the root directory is the
/// empty cluster 2.
pub fn fat32_writes(plan: DiskPlan, volume_id: u32) -> Result<Vec<Write>, PlanError> {
    let fat = fat32_geometry(plan)?;
    let base = plan.esp_first;
    let mut first_fat_sector = block(plan);
    put32(&mut first_fat_sector, 0, 0x0fff_fff8);
    put32(&mut first_fat_sector, 4, 0x0fff_ffff);
    put32(&mut first_fat_sector, 8, 0x0fff_ffff);
    let boot = boot_sector(plan, fat, volume_id);
    let info = fs_info(plan);
    let data_start = base + FAT32_RESERVED_SECTORS + 2 * fat.fat_sectors;
    let mut writes = vec![Write {
        lba: base,
        blocks: FAT32_RESERVED_SECTORS + 2 * fat.fat_sectors,
        fill: Fill::Zero,
    }];
    for (at, content) in [(0, &boot), (1, &info), (6, &boot), (7, &info)] {
        writes.push(Write {
            lba: base + at,
            blocks: 1,
            fill: Fill::Bytes(content.clone()),
        });
    }
    for copy in 0..2 {
        writes.push(Write {
            lba: base + FAT32_RESERVED_SECTORS + copy * fat.fat_sectors,
            blocks: 1,
            fill: Fill::Bytes(first_fat_sector.clone()),
        });
    }
    writes.push(Write {
        lba: data_start,
        blocks: fat.sectors_per_cluster,
        fill: Fill::Zero,
    });
    Ok(writes)
}

/// `EFI_LOAD_OPTION` for `Boot####`: active, `description`, short-form `HD()` node of the EFI
/// system partition (partition 1, GPT signature) then the loader's file path.
#[must_use]
pub fn load_option(
    description: &str,
    plan: DiskPlan,
    partition_guid: [u8; 16],
    path: &str,
) -> Vec<u8> {
    let mut hd = vec![0_u8; 42];
    hd[0] = 4;
    hd[1] = 1;
    put16(&mut hd, 2, 42);
    put32(&mut hd, 4, 1);
    put64(&mut hd, 8, plan.esp_first);
    put64(&mut hd, 16, plan.esp_blocks());
    hd[24..40].copy_from_slice(&partition_guid);
    hd[40] = 2;
    hd[41] = 2;
    let path16: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
    let mut file = vec![4_u8, 4, 0, 0];
    put16(&mut file, 2, (4 + 2 * path16.len()) as u16);
    for unit in &path16 {
        file.extend_from_slice(&unit.to_le_bytes());
    }
    let end = [0x7f_u8, 0xff, 0x04, 0x00];
    let list_len = hd.len() + file.len() + end.len();

    let mut out = Vec::new();
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&(list_len as u16).to_le_bytes());
    for unit in description.encode_utf16().chain(core::iter::once(0)) {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&hd);
    out.extend_from_slice(&file);
    out.extend_from_slice(&end);
    out
}

/// Description of a `Boot####` load option (UCS-2 after the 6-byte header), if well formed.
#[must_use]
pub fn load_option_description(option: &[u8]) -> Option<alloc::string::String> {
    let mut text = alloc::string::String::new();
    let mut at = 6;
    loop {
        let unit = u16::from_le_bytes([*option.get(at)?, *option.get(at + 1)?]);
        if unit == 0 {
            return Some(text);
        }
        text.push(char::from_u32(u32::from(unit))?);
        at += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn plan(block_size: u32, bytes: u64) -> DiskPlan {
        DiskPlan::new(block_size, bytes / u64::from(block_size)).unwrap()
    }

    fn apply(writes: &[Write], block_size: u64, image: &mut [u8]) {
        for write in writes {
            let start = (write.lba * block_size) as usize;
            let len = (write.blocks * block_size) as usize;
            match &write.fill {
                Fill::Zero => image[start..start + len].fill(0),
                Fill::Bytes(bytes) => {
                    assert_eq!(bytes.len(), len, "write at {}", write.lba);
                    image[start..start + len].copy_from_slice(bytes);
                }
            }
        }
    }

    #[test]
    fn plans_an_aligned_esp_and_rejects_small_disks() {
        let p = plan(512, GIB);
        assert_eq!(p.esp_first(), 2048);
        assert_eq!(p.esp_blocks() * 512, ESP_BYTES);
        assert_eq!(plan(4096, GIB).esp_first(), 256);
        assert_eq!(
            DiskPlan::new(512, ESP_BYTES / 512),
            Err(PlanError::DiskTooSmall)
        );
        assert_eq!(
            DiskPlan::new(520, 1 << 30),
            Err(PlanError::UnsupportedBlockSize)
        );
    }

    #[test]
    fn writes_a_valid_gpt() {
        for block_size in [512_u32, 4096] {
            let p = plan(block_size, GIB);
            let bs = u64::from(block_size);
            let mut image = vec![0xa5_u8; (GIB) as usize];
            apply(&gpt_writes(p, [1; 16], [2; 16]), bs, &mut image);
            assert_eq!(&image[510..512], &[0x55, 0xaa]);
            assert_eq!(image[446 + 4], 0xee);
            let header = &image[bs as usize..2 * bs as usize];
            assert!(gpt_header_valid(p, header));
            let backup = &image[((GIB / bs - 1) * bs) as usize..];
            assert_eq!(&backup[..8], b"EFI PART");
            let entries = &image[2 * bs as usize..2 * bs as usize + 16384];
            let entries_crc = u32::from_le_bytes([header[88], header[89], header[90], header[91]]);
            assert_eq!(crc32(entries), entries_crc);
            assert_eq!(&entries[..16], &ESP_TYPE_GUID);
            let mut corrupted = header.to_vec();
            corrupted[40] ^= 1;
            assert!(!gpt_header_valid(p, &corrupted));
        }
    }

    #[test]
    fn formats_fat32_with_enough_clusters() {
        let p = plan(512, GIB);
        let fat = fat32_geometry(p).unwrap();
        assert!(fat.clusters >= FAT32_MIN_CLUSTERS);
        assert_eq!(fat.sectors_per_cluster, 8);
        // The FATs hold every cluster plus the two reserved entries.
        assert!(fat.fat_sectors * 512 / 4 >= fat.clusters + 2);
        let mut image = vec![0xa5_u8; (p.esp_first() + p.esp_blocks()) as usize * 512];
        apply(&fat32_writes(p, 0x1234_5678).unwrap(), 512, &mut image);
        let boot = &image[2048 * 512..2049 * 512];
        assert_eq!(&boot[82..90], b"FAT32   ");
        assert_eq!(u16::from_le_bytes([boot[11], boot[12]]), 512);
        assert_eq!(&boot[510..512], &[0x55, 0xaa]);
        let backup = &image[(2048 + 6) * 512..(2048 + 7) * 512];
        assert_eq!(boot, backup);
        let fat0 = (2048 + 32) * 512;
        assert_eq!(&image[fat0..fat0 + 4], &0x0fff_fff8_u32.to_le_bytes());
        // Old bytes never survive in the FAT regions.
        let fat_end = fat0 + 2 * fat.fat_sectors as usize * 512;
        assert!(
            image[fat0 + 12..fat_end]
                .iter()
                .enumerate()
                .all(|(i, b)| { *b == 0 || (i + 12) % (fat.fat_sectors as usize * 512) < 12 })
        );
    }

    #[test]
    fn builds_a_load_option() {
        let p = plan(512, GIB);
        let option = load_option("omni-os", p, [7; 16], "\\EFI\\omni-os\\BOOTX64.EFI");
        assert_eq!(&option[..4], &1_u32.to_le_bytes());
        assert_eq!(load_option_description(&option).as_deref(), Some("omni-os"));
        let list_len = usize::from(u16::from_le_bytes([option[4], option[5]]));
        let list = &option[6 + 16..];
        assert_eq!(list.len(), list_len);
        assert_eq!((list[0], list[1], list[2]), (4, 1, 42));
        assert_eq!(&list[24..40], &[7; 16]);
        assert_eq!(&list[list.len() - 4..], &[0x7f, 0xff, 0x04, 0x00]);
    }
}
