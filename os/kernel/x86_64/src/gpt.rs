//! Minimal GPT partition-table reader (dossier section 12, roadmap Phase 3
//! "GPT parser/writer with safety checks").
//!
//! Reading a disk's own partition table is the first step of recognising and later
//! installing to a real disk. This reads the GPT header at LBA 1 off an AHCI disk,
//! checks the `EFI PART` signature and the header's own CRC32, then walks the first
//! sector of the partition entry array and reports the first real partition - the
//! EFI System Partition on a normal disk. It is read-only and safe on any disk;
//! a disk with no GPT (a bare FAT image, say) is reported unavailable, not failed.

use crate::ahci::{self, SECTOR_SIZE};
use crate::{debug_write, debug_write_hex_u64, debug_write_u64};

fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    ])
}

fn read_u64(buffer: &[u8], offset: usize) -> u64 {
    u64::from(read_u32(buffer, offset)) | (u64::from(read_u32(buffer, offset + 4)) << 32)
}

/// The EFI System Partition type GUID (C12A7328-F81F-11D2-BA4B-00A0C93EC93B), in
/// on-disk mixed-endian byte order.
const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];

/// CRC32 (IEEE, reflected) of `data`, as GPT uses for its header check.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn guid_is_zero(guid: &[u8]) -> bool {
    guid.iter().all(|&b| b == 0)
}

/// Read and validate the GPT off the first AHCI disk, and report its first
/// partition. Prints `AW_GPT_UNAVAILABLE` and returns when there is no AHCI disk
/// or the disk carries no GPT, so it is safe on every boot configuration.
pub fn prove() {
    debug_write("AW_GPT_BEGIN\n");
    let Some(port) = ahci::init() else {
        debug_write("AW_GPT_UNAVAILABLE reason=no_disk\n");
        return;
    };

    let mut header = [0u8; SECTOR_SIZE];
    if port.read_sector(1, &mut header).is_err() {
        debug_write("AW_GPT_UNAVAILABLE reason=read_header\n");
        return;
    }
    if &header[0..8] != b"EFI PART" {
        debug_write("AW_GPT_UNAVAILABLE reason=no_gpt\n");
        return;
    }

    // Verify the header CRC32: the field is computed with its own four bytes
    // zeroed, over header_size bytes (bytes 12..16).
    let header_size = read_u32(&header, 12) as usize;
    let stored_crc = read_u32(&header, 16);
    if !(92..=SECTOR_SIZE).contains(&header_size) {
        debug_write("AW_GPT_FAIL reason=bad_header_size\n");
        return;
    }
    let mut scratch = header;
    scratch[16] = 0;
    scratch[17] = 0;
    scratch[18] = 0;
    scratch[19] = 0;
    let computed = crc32(&scratch[0..header_size]);
    if computed != stored_crc {
        debug_write("AW_GPT_FAIL reason=header_crc stored=");
        debug_write_hex_u64(u64::from(stored_crc));
        debug_write(" computed=");
        debug_write_hex_u64(u64::from(computed));
        debug_write("\n");
        return;
    }
    debug_write("AW_GPT_HEADER_OK\n");

    let entries_lba = read_u64(&header, 72);
    let entry_count = read_u32(&header, 80);
    let entry_size = read_u32(&header, 84) as usize;
    debug_write("AW_GPT_TABLE entries=");
    debug_write_u64(u64::from(entry_count));
    debug_write(" entry_size=");
    debug_write_u64(entry_size as u64);
    debug_write("\n");
    if !(128..=SECTOR_SIZE).contains(&entry_size) {
        debug_write("AW_GPT_FAIL reason=bad_entry_size\n");
        return;
    }

    // The first sector of the entry array is enough to find the first partition.
    let mut table = [0u8; SECTOR_SIZE];
    if port.read_sector(entries_lba, &mut table).is_err() {
        debug_write("AW_GPT_FAIL reason=read_entries\n");
        return;
    }

    let per_sector = SECTOR_SIZE / entry_size;
    for index in 0..per_sector.min(entry_count as usize) {
        let base = index * entry_size;
        let type_guid = &table[base..base + 16];
        if guid_is_zero(type_guid) {
            continue; // unused entry
        }
        let first_lba = read_u64(&table, base + 32);
        let last_lba = read_u64(&table, base + 40);
        let is_esp = type_guid == ESP_TYPE_GUID;
        debug_write("AW_GPT_PARTITION index=");
        debug_write_u64(index as u64);
        debug_write(" first_lba=");
        debug_write_u64(first_lba);
        debug_write(" last_lba=");
        debug_write_u64(last_lba);
        debug_write(if is_esp { " esp=1\n" } else { " esp=0\n" });
        debug_write("AW_GPT_PROOF_OK\n");
        // Read a file from the ESP's own FAT16 filesystem, at its partition offset:
        // the full storage stack (AHCI -> GPT -> partition -> FAT -> file) on a real
        // disk layout, the way an installed system's files are reached.
        if is_esp {
            crate::fat16::prove_partition(&port, first_lba);
        }
        return;
    }

    debug_write("AW_GPT_FAIL reason=no_partition\n");
}

// ---- GPT write (roadmap Phase 3 "GPT parser/writer") -----------------------
//
// The reader recognises an existing disk; writing a partition table is the other
// half an installer needs: lay down a protective MBR, a primary and backup GPT
// header (each with its own CRC32), and a partition entry array carrying one EFI
// System Partition. It is gated and only ever pointed at a scratch disk, since it
// overwrites the partition table.

#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
use alloc::vec;
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
use crate::fat16::{SectorSink, SectorSource};

/// A fixed disk GUID for the scratch disk we build (value is irrelevant to the
/// proof, only that it round-trips and is non-zero).
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
const DISK_GUID: [u8; 16] = [
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01,
];
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
const PART_GUID: [u8; 16] = [
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f, 0x90,
];

#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
const NUM_ENTRIES: usize = 128;
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
const ENTRY_SIZE: usize = 128;
/// 128 entries x 128 bytes = 16 KiB = 32 sectors.
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
const ENTRY_SECTORS: u64 = (NUM_ENTRIES * ENTRY_SIZE / SECTOR_SIZE) as u64;

#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
fn put_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
fn put_u64(buffer: &mut [u8], offset: usize, value: u64) {
    buffer[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Fill in one GPT header (92 bytes) at `my_lba`, its alternate at `alt_lba`, with
/// the partition-array CRC already computed. Leaves the header CRC field zeroed,
/// computes it over `header_size` bytes, and stores it.
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
fn build_header(
    my_lba: u64,
    alt_lba: u64,
    entries_lba: u64,
    first_usable: u64,
    last_usable: u64,
    array_crc: u32,
) -> [u8; SECTOR_SIZE] {
    let mut header = [0u8; SECTOR_SIZE];
    header[0..8].copy_from_slice(b"EFI PART");
    put_u32(&mut header, 8, 0x0001_0000); // revision 1.0
    put_u32(&mut header, 12, 92); // header size
    // 16..20 header CRC (zero for now)
    put_u64(&mut header, 24, my_lba);
    put_u64(&mut header, 32, alt_lba);
    put_u64(&mut header, 40, first_usable);
    put_u64(&mut header, 48, last_usable);
    header[56..72].copy_from_slice(&DISK_GUID);
    put_u64(&mut header, 72, entries_lba);
    put_u32(&mut header, 80, NUM_ENTRIES as u32);
    put_u32(&mut header, 84, ENTRY_SIZE as u32);
    put_u32(&mut header, 88, array_crc);
    let crc = crc32(&header[0..92]);
    put_u32(&mut header, 16, crc);
    header
}

/// Write a GPT to a blank disk of `total_sectors`, with one ESP spanning the whole
/// usable area. Lays down the protective MBR, the primary header and entry array
/// near the front, and the backup array and header at the end of the disk.
#[cfg(any(feature = "gpt-write-smoke-test", feature = "disk-build-smoke-test"))]
pub fn write_table<S: SectorSink>(sink: &S, total_sectors: u64) -> Result<(), &'static str> {
    if total_sectors < 2 * ENTRY_SECTORS + 8 {
        return Err("disk_too_small");
    }
    let last_lba = total_sectors - 1;
    let primary_entries_lba = 2u64;
    let backup_entries_lba = last_lba - ENTRY_SECTORS; // 32 sectors before backup header
    let first_usable = primary_entries_lba + ENTRY_SECTORS;
    let last_usable = backup_entries_lba - 1;

    // Protective MBR at LBA 0: one 0xEE partition covering the disk.
    let mut mbr = [0u8; SECTOR_SIZE];
    let entry = 446;
    mbr[entry] = 0x00; // not bootable
    mbr[entry + 1] = 0x00;
    mbr[entry + 2] = 0x02;
    mbr[entry + 3] = 0x00; // start CHS
    mbr[entry + 4] = 0xee; // type: GPT protective
    mbr[entry + 5] = 0xff;
    mbr[entry + 6] = 0xff;
    mbr[entry + 7] = 0xff; // end CHS
    put_u32(&mut mbr, entry + 8, 1); // first LBA
    let size = core::cmp::min(total_sectors - 1, u64::from(u32::MAX)) as u32;
    put_u32(&mut mbr, entry + 12, size);
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    sink.write_sector(0, &mbr)?;

    // Partition entry array: one ESP entry, the rest zero.
    let mut array = vec![0u8; NUM_ENTRIES * ENTRY_SIZE];
    array[0..16].copy_from_slice(&ESP_TYPE_GUID);
    array[16..32].copy_from_slice(&PART_GUID);
    put_u64(&mut array, 32, first_usable);
    put_u64(&mut array, 40, last_usable);
    // 48..56 attributes = 0. Name (UTF-16LE "EFI System") at 56.
    for (i, ch) in "EFI System".encode_utf16().enumerate() {
        array[56 + i * 2..56 + i * 2 + 2].copy_from_slice(&ch.to_le_bytes());
    }
    let array_crc = crc32(&array);

    // Write both copies of the entry array, one sector at a time.
    for i in 0..ENTRY_SECTORS {
        let mut sector = [0u8; SECTOR_SIZE];
        let start = (i as usize) * SECTOR_SIZE;
        sector.copy_from_slice(&array[start..start + SECTOR_SIZE]);
        sink.write_sector(primary_entries_lba + i, &sector)?;
        sink.write_sector(backup_entries_lba + i, &sector)?;
    }

    // Primary header at LBA 1 (alternate = backup at last LBA), backup at last LBA.
    let primary = build_header(1, last_lba, primary_entries_lba, first_usable, last_usable, array_crc);
    sink.write_sector(1, &primary)?;
    let backup = build_header(last_lba, 1, backup_entries_lba, first_usable, last_usable, array_crc);
    sink.write_sector(last_lba, &backup)?;
    Ok(())
}

/// Prove a GPT write: partition a blank scratch disk, then read the table back
/// through the ordinary reader logic and confirm the header CRC32 validates and the
/// first partition is the ESP at the expected offset. Scratch disk only. Emits
/// `AW_GPTWRITE_*`. `total_sectors` must match the scratch disk's real size.
#[cfg(feature = "gpt-write-smoke-test")]
pub fn prove_write<S: SectorSource + SectorSink>(source: &S, total_sectors: u64) {
    debug_write("AW_GPTWRITE_BEGIN sectors=");
    debug_write_u64(total_sectors);
    debug_write("\n");

    if let Err(reason) = write_table(source, total_sectors) {
        debug_write("AW_GPTWRITE_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_GPTWRITE_WROTE\n");

    // Read the primary header back and validate exactly as the reader does.
    let mut header = [0u8; SECTOR_SIZE];
    if source.read_sector(1, &mut header).is_err() {
        debug_write("AW_GPTWRITE_FAIL reason=reread_header\n");
        return;
    }
    if &header[0..8] != b"EFI PART" {
        debug_write("AW_GPTWRITE_FAIL reason=no_signature\n");
        return;
    }
    let header_size = read_u32(&header, 12) as usize;
    let stored_crc = read_u32(&header, 16);
    let mut scratch = header;
    scratch[16] = 0;
    scratch[17] = 0;
    scratch[18] = 0;
    scratch[19] = 0;
    if crc32(&scratch[0..header_size]) != stored_crc {
        debug_write("AW_GPTWRITE_FAIL reason=header_crc\n");
        return;
    }
    debug_write("AW_GPTWRITE_HEADER_OK\n");

    // Validate the backup header at the alternate LBA too.
    let alt_lba = read_u64(&header, 32);
    let mut backup = [0u8; SECTOR_SIZE];
    if source.read_sector(alt_lba, &mut backup).is_err() || &backup[0..8] != b"EFI PART" {
        debug_write("AW_GPTWRITE_FAIL reason=backup_header\n");
        return;
    }
    let backup_size = read_u32(&backup, 12) as usize;
    let backup_crc = read_u32(&backup, 16);
    let mut bscratch = backup;
    bscratch[16] = 0;
    bscratch[17] = 0;
    bscratch[18] = 0;
    bscratch[19] = 0;
    if crc32(&bscratch[0..backup_size]) != backup_crc {
        debug_write("AW_GPTWRITE_FAIL reason=backup_crc\n");
        return;
    }
    debug_write("AW_GPTWRITE_BACKUP_OK\n");

    // The first entry must be the ESP we wrote.
    let entries_lba = read_u64(&header, 72);
    let mut table = [0u8; SECTOR_SIZE];
    if source.read_sector(entries_lba, &mut table).is_err() {
        debug_write("AW_GPTWRITE_FAIL reason=reread_entries\n");
        return;
    }
    if table[0..16] != ESP_TYPE_GUID {
        debug_write("AW_GPTWRITE_FAIL reason=not_esp\n");
        return;
    }
    let first_lba = read_u64(&table, 32);
    debug_write("AW_GPTWRITE_ESP first_lba=");
    debug_write_u64(first_lba);
    debug_write("\n");
    debug_write("AW_GPTWRITE_PROOF_OK\n");
}

/// Read and validate the GPT on `source` and return the first real partition's
/// (first_lba, last_lba), or [`None`] if there is no valid GPT or no partition.
/// The reusable discovery step behind the disk builder's full-stack read.
#[cfg(feature = "disk-build-smoke-test")]
pub fn find_first_partition<S: SectorSource>(source: &S) -> Option<(u64, u64)> {
    let mut header = [0u8; SECTOR_SIZE];
    source.read_sector(1, &mut header).ok()?;
    if &header[0..8] != b"EFI PART" {
        return None;
    }
    let header_size = read_u32(&header, 12) as usize;
    if !(92..=SECTOR_SIZE).contains(&header_size) {
        return None;
    }
    let stored_crc = read_u32(&header, 16);
    let mut scratch = header;
    scratch[16] = 0;
    scratch[17] = 0;
    scratch[18] = 0;
    scratch[19] = 0;
    if crc32(&scratch[0..header_size]) != stored_crc {
        return None;
    }
    let entries_lba = read_u64(&header, 72);
    let entry_count = read_u32(&header, 80);
    let entry_size = read_u32(&header, 84) as usize;
    if !(128..=SECTOR_SIZE).contains(&entry_size) {
        return None;
    }
    let mut table = [0u8; SECTOR_SIZE];
    source.read_sector(entries_lba, &mut table).ok()?;
    let per_sector = SECTOR_SIZE / entry_size;
    for index in 0..per_sector.min(entry_count as usize) {
        let base = index * entry_size;
        if guid_is_zero(&table[base..base + 16]) {
            continue;
        }
        let first_lba = read_u64(&table, base + 32);
        let last_lba = read_u64(&table, base + 40);
        return Some((first_lba, last_lba));
    }
    None
}
