//! Read-only FAT16 file read from a virtio-block disk (dossier section 12,
//! roadmap P0 step 7).
//!
//! This is the first filesystem: it parses the FAT16 BIOS parameter block on the
//! virtio disk, walks the root directory for a known 8.3 file, follows its
//! cluster chain through the FAT, and returns the bytes - proving a real file
//! read end to end, device through filesystem. It uses `alloc` (the kernel
//! heap) for the FAT and the assembled file, so it also exercises that path.
//!
//! FAT is little-endian, matching the guest. Only what this proof needs is
//! implemented: no long names, no writing, no subdirectories.

use alloc::vec::Vec;

use crate::virtio_blk::{BlkDevice, SECTOR_SIZE};
use crate::{debug_write, debug_write_u64};

/// A source of 512-byte sectors, so the FAT reader works on any block device: a
/// whole disk (`base_lba` 0) or a partition at some LBA offset. Both the virtio
/// block device and an AHCI port expose exactly this.
pub trait SectorSource {
    fn read_sector(&self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str>;
}

impl SectorSource for BlkDevice {
    fn read_sector(&self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        BlkDevice::read_sector(self, lba, out)
    }
}

impl SectorSource for crate::ahci::AhciPort {
    fn read_sector(&self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        crate::ahci::AhciPort::read_sector(self, lba, out)
    }
}

/// The file the test disk carries, as an 8.3 directory name and its contents.
const TARGET_NAME: &[u8; 11] = b"HELLO   TXT";
const EXPECTED: &[u8] = b"ACCESSIBLE-WINDOWS-FS-OK\n";

fn read_u16(buffer: &[u8], offset: usize) -> u16 {
    u16::from(buffer[offset]) | (u16::from(buffer[offset + 1]) << 8)
}

fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from(buffer[offset])
        | (u32::from(buffer[offset + 1]) << 8)
        | (u32::from(buffer[offset + 2]) << 16)
        | (u32::from(buffer[offset + 3]) << 24)
}

fn slices_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

/// Where a FAT16 volume's regions start, read from its boot sector.
struct Geometry {
    sector_size: usize,
    sectors_per_cluster: usize,
    fat_start: usize,
    root_start: usize,
    root_sectors: usize,
    data_start: usize,
    fat_sectors: usize,
    // Read only by the scratch-disk write proofs (cluster allocation bound).
    #[cfg_attr(
        not(any(
            feature = "fat-write-smoke-test",
            feature = "fat-format-smoke-test",
            feature = "disk-build-smoke-test"
        )),
        allow(dead_code)
    )]
    data_clusters: usize,
}

fn parse_geometry(boot: &[u8; SECTOR_SIZE]) -> Option<Geometry> {
    let sector_size = read_u16(boot, 0x0b) as usize;
    let sectors_per_cluster = boot[0x0d] as usize;
    let reserved = read_u16(boot, 0x0e) as usize;
    let num_fats = boot[0x10] as usize;
    let root_entries = read_u16(boot, 0x11) as usize;
    let fat_sectors = read_u16(boot, 0x16) as usize;
    let total_sectors_16 = read_u16(boot, 0x13) as usize;
    let total_sectors = if total_sectors_16 != 0 {
        total_sectors_16
    } else {
        read_u32(boot, 0x20) as usize
    };
    if sector_size != SECTOR_SIZE || sectors_per_cluster == 0 || fat_sectors == 0 || num_fats == 0 {
        return None;
    }
    let fat_start = reserved;
    let root_start = reserved.checked_add(num_fats.checked_mul(fat_sectors)?)?;
    let root_sectors = (root_entries * 32).div_ceil(sector_size);
    let data_start = root_start.checked_add(root_sectors)?;
    let data_sectors = total_sectors.checked_sub(data_start)?;
    let data_clusters = data_sectors / sectors_per_cluster;
    let fat_entries = fat_sectors.checked_mul(sector_size)? / 2;
    if data_clusters == 0 || data_clusters.checked_add(2)? > fat_entries {
        return None;
    }
    Some(Geometry {
        sector_size,
        sectors_per_cluster,
        fat_start,
        root_start,
        root_sectors,
        data_start,
        fat_sectors,
        data_clusters,
    })
}

/// Scan the root directory for the 8.3 `name`, returning (first cluster, size).
fn find_file<S: SectorSource>(
    source: &S,
    base_lba: u64,
    geometry: &Geometry,
    name: &[u8; 11],
) -> Result<Option<(u32, u32)>, &'static str> {
    let mut sector = [0u8; SECTOR_SIZE];
    for index in 0..geometry.root_sectors {
        source
            .read_sector(base_lba + (geometry.root_start + index) as u64, &mut sector)
            .map_err(|_| "read_root")?;
        let mut offset = 0;
        while offset < geometry.sector_size {
            let entry = &sector[offset..offset + 32];
            match entry[0] {
                0x00 => return Ok(None), // no further entries
                0xe5 => {}               // deleted
                _ if entry[11] == 0x0f => {} // long-name entry
                _ if slices_equal(&entry[0..11], name) => {
                    let first_cluster =
                        u32::from(read_u16(entry, 0x1a)) | (u32::from(read_u16(entry, 0x14)) << 16);
                    let size = read_u32(entry, 0x1c);
                    return Ok(Some((first_cluster, size)));
                }
                _ => {}
            }
            offset += 32;
        }
    }
    Ok(None)
}

/// Read the whole file by following its cluster chain through the FAT.
fn read_file<S: SectorSource>(
    source: &S,
    base_lba: u64,
    geometry: &Geometry,
    first_cluster: u32,
    size: u32,
) -> Result<Vec<u8>, &'static str> {
    // Load the FAT so the chain can be followed without re-reading sectors.
    let mut fat = Vec::new();
    let mut sector = [0u8; SECTOR_SIZE];
    for index in 0..geometry.fat_sectors {
        source
            .read_sector(base_lba + (geometry.fat_start + index) as u64, &mut sector)
            .map_err(|_| "read_fat")?;
        fat.extend_from_slice(&sector);
    }

    let mut contents = Vec::new();
    let mut cluster = first_cluster;
    let mut guard = 0u32;
    while (2..0xfff8).contains(&cluster) && contents.len() < size as usize {
        let first_sector = geometry.data_start + (cluster as usize - 2) * geometry.sectors_per_cluster;
        for index in 0..geometry.sectors_per_cluster {
            source
                .read_sector(base_lba + (first_sector + index) as u64, &mut sector)
                .map_err(|_| "read_data")?;
            contents.extend_from_slice(&sector);
        }
        let entry = (cluster as usize) * 2;
        cluster = read_u16(&fat, entry) as u32;
        guard += 1;
        if guard > 4096 {
            return Err("chain_too_long");
        }
    }
    contents.truncate(size as usize);
    Ok(contents)
}

/// Read a whole file named by its 8.3 directory name (e.g. `b"USERPROGELF"`),
/// returning its bytes, or [`None`] if the volume cannot be parsed or the file is
/// absent. The reusable entry point behind [`prove`], used by the userland loader.
pub fn load_file(device: &BlkDevice, name: &[u8; 11]) -> Option<Vec<u8>> {
    let mut boot = [0u8; SECTOR_SIZE];
    device.read_sector(0, &mut boot).ok()?;
    let geometry = parse_geometry(&boot)?;
    let (first_cluster, size) = find_file(device, 0, &geometry, name).ok()??;
    read_file(device, 0, &geometry, first_cluster, size).ok()
}

/// Prove a file read from a FAT16 filesystem on the virtio disk.
pub fn prove(device: &BlkDevice) {
    debug_write("AW_FS_BEGIN\n");

    let mut boot = [0u8; SECTOR_SIZE];
    if device.read_sector(0, &mut boot).is_err() {
        debug_write("AW_FS_FAIL reason=read_boot\n");
        return;
    }
    let Some(geometry) = parse_geometry(&boot) else {
        debug_write("AW_FS_FAIL reason=bad_bpb\n");
        return;
    };

    let file = match find_file(device, 0, &geometry, TARGET_NAME) {
        Ok(Some(file)) => file,
        Ok(None) => {
            debug_write("AW_FS_FAIL reason=not_found\n");
            return;
        }
        Err(reason) => {
            debug_write("AW_FS_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };
    let (first_cluster, size) = file;
    debug_write("AW_FS_FILE_FOUND size=");
    debug_write_u64(u64::from(size));
    debug_write("\n");

    let contents = match read_file(device, 0, &geometry, first_cluster, size) {
        Ok(contents) => contents,
        Err(reason) => {
            debug_write("AW_FS_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };

    if slices_equal(&contents, EXPECTED) {
        debug_write("AW_FS_READ_OK\n");
        debug_write("AW_FS_PROOF_OK\n");
    } else {
        debug_write("AW_FS_FAIL reason=content_mismatch\n");
    }
}

/// Prove a file read from a FAT16 filesystem that starts at `base_lba` on any
/// sector source - e.g. the ESP located on a real GPT disk over AHCI. Reports
/// unavailable (not failed) if the partition is not FAT16, so it is safe to try on
/// any partition. Emits `AW_FSPART_*` markers.
pub fn prove_partition<S: SectorSource>(source: &S, base_lba: u64) {
    debug_write("AW_FSPART_BEGIN base_lba=");
    debug_write_u64(base_lba);
    debug_write("\n");

    let mut boot = [0u8; SECTOR_SIZE];
    if source.read_sector(base_lba, &mut boot).is_err() {
        debug_write("AW_FSPART_UNAVAILABLE reason=read_boot\n");
        return;
    }
    let Some(geometry) = parse_geometry(&boot) else {
        debug_write("AW_FSPART_UNAVAILABLE reason=not_fat16\n");
        return;
    };

    let (first_cluster, size) = match find_file(source, base_lba, &geometry, TARGET_NAME) {
        Ok(Some(file)) => file,
        Ok(None) => {
            debug_write("AW_FSPART_UNAVAILABLE reason=not_found\n");
            return;
        }
        Err(reason) => {
            debug_write("AW_FSPART_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };

    match read_file(source, base_lba, &geometry, first_cluster, size) {
        Ok(contents) if slices_equal(&contents, EXPECTED) => {
            debug_write("AW_FSPART_READ_OK\n");
            debug_write("AW_FSPART_PROOF_OK\n");
        }
        Ok(_) => debug_write("AW_FSPART_FAIL reason=content_mismatch\n"),
        Err(reason) => {
            debug_write("AW_FSPART_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
        }
    }
}

// ---- FAT16 write (roadmap Phase 3 "filesystem write") ----------------------
//
// Creating a file is what an installer needs: allocate a free cluster, write the
// data, chain the FAT (every copy), and add a root-directory entry. Kept to a
// single-cluster file, the size the install/recovery bootstrap needs, and gated so
// it only ever runs against a scratch disk - it modifies the filesystem.

/// A sink for 512-byte sectors, the write counterpart of [`SectorSource`]. Shared
/// by the filesystem writer and the GPT writer.
#[cfg(any(feature = "fat-write-smoke-test", feature = "gpt-write-smoke-test", feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
pub trait SectorSink {
    fn write_sector(&self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), &'static str>;
}

#[cfg(any(feature = "fat-write-smoke-test", feature = "gpt-write-smoke-test", feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
impl SectorSink for crate::ahci::AhciPort {
    fn write_sector(&self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        crate::ahci::AhciPort::write_sector(self, lba, data)
    }
}

#[cfg(any(feature = "fat-write-smoke-test", feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
fn put_u16(buffer: &mut [u8], offset: usize, value: u16) {
    buffer[offset] = value as u8;
    buffer[offset + 1] = (value >> 8) as u8;
}

/// Create a single-cluster file `name` with `data` on a FAT16 volume at `base_lba`.
///
/// Allocates the first free cluster, writes the data into it, marks the cluster as
/// end-of-chain in every FAT copy, and writes a root-directory entry. Fails if the
/// file needs more than one cluster, or there is no free cluster or root slot.
#[cfg(any(feature = "fat-write-smoke-test", feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
pub fn write_file<S: SectorSource + SectorSink>(
    source: &S,
    base_lba: u64,
    name: &[u8; 11],
    data: &[u8],
) -> Result<(), &'static str> {
    let mut boot = [0u8; SECTOR_SIZE];
    source.read_sector(base_lba, &mut boot).map_err(|_| "read_boot")?;
    let geometry = parse_geometry(&boot).ok_or("bad_bpb")?;
    let num_fats = (geometry.root_start - geometry.fat_start) / geometry.fat_sectors;
    let cluster_bytes = geometry.sectors_per_cluster * SECTOR_SIZE;
    if data.len() > cluster_bytes {
        return Err("file_too_big");
    }

    // Find the first free FAT entry (value 0) at cluster index >= 2.
    let entries_per_sector = SECTOR_SIZE / 2;
    let max_cluster_exclusive = 2usize
        .checked_add(geometry.data_clusters)
        .ok_or("geometry_overflow")?;
    let mut free_cluster = 0usize;
    let mut sector = [0u8; SECTOR_SIZE];
    'scan: for fat_sector in 0..geometry.fat_sectors {
        source
            .read_sector(base_lba + (geometry.fat_start + fat_sector) as u64, &mut sector)
            .map_err(|_| "read_fat")?;
        for i in 0..entries_per_sector {
            let cluster = fat_sector * entries_per_sector + i;
            if cluster < 2 {
                continue;
            }
            if cluster >= max_cluster_exclusive {
                break 'scan;
            }
            if read_u16(&sector, i * 2) == 0 {
                free_cluster = cluster;
                break 'scan;
            }
        }
    }
    if free_cluster == 0 {
        return Err("no_free_cluster");
    }

    // Write the data into that cluster, one sector at a time, zero-padded.
    let first_sector = geometry.data_start + (free_cluster - 2) * geometry.sectors_per_cluster;
    for k in 0..geometry.sectors_per_cluster {
        let mut buf = [0u8; SECTOR_SIZE];
        let start = k * SECTOR_SIZE;
        if start < data.len() {
            let end = (start + SECTOR_SIZE).min(data.len());
            buf[..end - start].copy_from_slice(&data[start..end]);
        }
        source
            .write_sector(base_lba + (first_sector + k) as u64, &buf)
            .map_err(|_| "write_data")?;
    }

    // Mark the cluster end-of-chain (0xFFFF) in every FAT copy.
    let fat_byte = free_cluster * 2;
    let fat_sector_index = fat_byte / SECTOR_SIZE;
    let fat_in_sector = fat_byte % SECTOR_SIZE;
    for copy in 0..num_fats {
        let lba = base_lba + (geometry.fat_start + copy * geometry.fat_sectors + fat_sector_index) as u64;
        source.read_sector(lba, &mut sector).map_err(|_| "read_fat_rw")?;
        put_u16(&mut sector, fat_in_sector, 0xffff);
        source.write_sector(lba, &sector).map_err(|_| "write_fat")?;
    }

    // Write a root-directory entry into the first free slot.
    for root_sector in 0..geometry.root_sectors {
        let lba = base_lba + (geometry.root_start + root_sector) as u64;
        source.read_sector(lba, &mut sector).map_err(|_| "read_root_rw")?;
        let mut offset = 0;
        while offset < SECTOR_SIZE {
            if sector[offset] == 0x00 || sector[offset] == 0xe5 {
                for byte in &mut sector[offset..offset + 32] {
                    *byte = 0;
                }
                sector[offset..offset + 11].copy_from_slice(name);
                sector[offset + 11] = 0x20; // attribute: archive
                put_u16(&mut sector, offset + 20, (free_cluster >> 16) as u16); // cluster high
                put_u16(&mut sector, offset + 26, free_cluster as u16); // cluster low
                let size = data.len() as u32;
                sector[offset + 28] = size as u8;
                sector[offset + 29] = (size >> 8) as u8;
                sector[offset + 30] = (size >> 16) as u8;
                sector[offset + 31] = (size >> 24) as u8;
                source.write_sector(lba, &sector).map_err(|_| "write_root")?;
                return Ok(());
            }
            offset += 32;
        }
    }
    Err("no_root_slot")
}

/// Prove a FAT16 write: create a file, then read it back through the normal read
/// path and confirm the bytes. Scratch disk only. Emits `AW_FATWRITE_*`.
#[cfg(feature = "fat-write-smoke-test")]
pub fn prove_fat_write<S: SectorSource + SectorSink>(source: &S, base_lba: u64) {
    debug_write("AW_FATWRITE_BEGIN\n");
    const NAME: &[u8; 11] = b"NEWFILE TXT";
    const CONTENT: &[u8] = b"AW-FAT-WRITE-OK\n";

    if let Err(reason) = write_file(source, base_lba, NAME, CONTENT) {
        debug_write("AW_FATWRITE_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_FATWRITE_WROTE\n");

    // Read it back through the ordinary reader.
    let mut boot = [0u8; SECTOR_SIZE];
    if source.read_sector(base_lba, &mut boot).is_err() {
        debug_write("AW_FATWRITE_FAIL reason=reread_boot\n");
        return;
    }
    let Some(geometry) = parse_geometry(&boot) else {
        debug_write("AW_FATWRITE_FAIL reason=reread_bpb\n");
        return;
    };
    let found = find_file(source, base_lba, &geometry, NAME);
    let (first_cluster, size) = match found {
        Ok(Some(file)) => file,
        _ => {
            debug_write("AW_FATWRITE_FAIL reason=not_found_after_write\n");
            return;
        }
    };
    match read_file(source, base_lba, &geometry, first_cluster, size) {
        Ok(contents) if slices_equal(&contents, CONTENT) => {
            debug_write("AW_FATWRITE_READBACK_OK\n");
            debug_write("AW_FATWRITE_PROOF_OK\n");
        }
        Ok(_) => debug_write("AW_FATWRITE_FAIL reason=content_mismatch\n"),
        Err(reason) => {
            debug_write("AW_FATWRITE_FAIL reason=");
            debug_write(reason);
            debug_write("\n");
        }
    }
}

// ---- FAT16 format / mkfs (roadmap Phase 3 "filesystem create") -------------
//
// Writing a file needed an existing filesystem; creating the filesystem itself is
// what an installer does to a fresh partition. Lay down a valid FAT16 boot sector
// (BPB), two zeroed FATs with their reserved first two entries, and a zeroed root
// directory. Combined with the GPT writer, the kernel can now build a whole disk
// from blank. Gated, scratch disk only: it overwrites the volume.

/// Format `total_sectors` starting at `base_lba` as an empty FAT16 volume. Chooses
/// one sector per cluster and 512 root entries, sizes the FAT to cover the data
/// area, and fails if the resulting cluster count is not in the FAT16 range.
#[cfg(any(feature = "fat-format-smoke-test", feature = "disk-build-smoke-test"))]
pub fn format<S: SectorSink>(
    sink: &S,
    base_lba: u64,
    total_sectors: u64,
) -> Result<(), &'static str> {
    const SPC: usize = 1; // sectors per cluster
    const RESERVED: usize = 1;
    const NUM_FATS: usize = 2;
    const ROOT_ENTRIES: usize = 512;
    let root_sectors = (ROOT_ENTRIES * 32).div_ceil(SECTOR_SIZE); // 32
    let total = total_sectors as usize;
    if total < RESERVED + root_sectors + 8 {
        return Err("disk_too_small");
    }
    // Microsoft FAT-spec FAT-size computation for FAT16 (256 = bytes_per_sec / 2).
    let tmp1 = total - (RESERVED + root_sectors);
    let tmp2 = 256 * SPC + NUM_FATS;
    let fat_sectors = tmp1.div_ceil(tmp2);
    let data_sectors = total
        .checked_sub(RESERVED + NUM_FATS * fat_sectors + root_sectors)
        .ok_or("geometry_overflow")?;
    let clusters = data_sectors / SPC;
    if !(4085..65525).contains(&clusters) {
        return Err("not_fat16_range");
    }

    // Boot sector / BPB.
    let mut boot = [0u8; SECTOR_SIZE];
    boot[0] = 0xeb;
    boot[1] = 0x3c;
    boot[2] = 0x90; // jump
    boot[3..11].copy_from_slice(b"AWKERNEL");
    put_u16(&mut boot, 0x0b, SECTOR_SIZE as u16);
    boot[0x0d] = SPC as u8;
    put_u16(&mut boot, 0x0e, RESERVED as u16);
    boot[0x10] = NUM_FATS as u8;
    put_u16(&mut boot, 0x11, ROOT_ENTRIES as u16);
    if total < 0x1_0000 {
        put_u16(&mut boot, 0x13, total as u16);
    }
    boot[0x15] = 0xf8; // media descriptor (fixed disk)
    put_u16(&mut boot, 0x16, fat_sectors as u16);
    put_u16(&mut boot, 0x18, 32); // sectors per track (nominal)
    put_u16(&mut boot, 0x1a, 8); // heads (nominal)
    boot[0x1c..0x20].copy_from_slice(&(base_lba as u32).to_le_bytes()); // hidden sectors
    if total >= 0x1_0000 {
        boot[0x20..0x24].copy_from_slice(&(total as u32).to_le_bytes());
    }
    boot[0x24] = 0x80; // BIOS drive number
    boot[0x26] = 0x29; // extended boot signature
    boot[0x27..0x2b].copy_from_slice(&0x4157_5f31u32.to_le_bytes()); // volume id
    boot[0x2b..0x36].copy_from_slice(b"AWDISK     "); // 11-byte volume label
    boot[0x36..0x3e].copy_from_slice(b"FAT16   "); // 8-byte fs type
    boot[510] = 0x55;
    boot[511] = 0xaa;
    sink.write_sector(base_lba, &boot)?;

    // Both FATs: first sector carries the two reserved entries, the rest are zero.
    let mut first_fat = [0u8; SECTOR_SIZE];
    put_u16(&mut first_fat, 0, 0xfff8); // FAT[0] = media | 0xFF00
    put_u16(&mut first_fat, 2, 0xffff); // FAT[1] = end-of-chain
    let zero = [0u8; SECTOR_SIZE];
    for f in 0..NUM_FATS {
        let fat_base = base_lba + (RESERVED + f * fat_sectors) as u64;
        sink.write_sector(fat_base, &first_fat)?;
        for s in 1..fat_sectors {
            sink.write_sector(fat_base + s as u64, &zero)?;
        }
    }

    // Root directory: all zero, so the first entry reads as end-of-directory.
    let root_base = base_lba + (RESERVED + NUM_FATS * fat_sectors) as u64;
    for s in 0..root_sectors {
        sink.write_sector(root_base + s as u64, &zero)?;
    }
    Ok(())
}

/// Prove a FAT16 format: format a blank scratch volume, create a file in it with the
/// existing writer, then read it back through the ordinary reader. Scratch disk
/// only. Emits `AW_MKFS_*`.
#[cfg(feature = "fat-format-smoke-test")]
pub fn prove_format<S: SectorSource + SectorSink>(source: &S, base_lba: u64, total_sectors: u64) {
    debug_write("AW_MKFS_BEGIN sectors=");
    debug_write_u64(total_sectors);
    debug_write("\n");

    if let Err(reason) = format(source, base_lba, total_sectors) {
        debug_write("AW_MKFS_FAIL reason=");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_MKFS_FORMATTED\n");

    const NAME: &[u8; 11] = b"MKFSFILETXT";
    const CONTENT: &[u8] = b"AW-MKFS-OK\n";
    if let Err(reason) = write_file(source, base_lba, NAME, CONTENT) {
        debug_write("AW_MKFS_FAIL reason=write_");
        debug_write(reason);
        debug_write("\n");
        return;
    }

    let mut boot = [0u8; SECTOR_SIZE];
    if source.read_sector(base_lba, &mut boot).is_err() {
        debug_write("AW_MKFS_FAIL reason=reread_boot\n");
        return;
    }
    let Some(geometry) = parse_geometry(&boot) else {
        debug_write("AW_MKFS_FAIL reason=reread_bpb\n");
        return;
    };
    let (first_cluster, size) = match find_file(source, base_lba, &geometry, NAME) {
        Ok(Some(file)) => file,
        _ => {
            debug_write("AW_MKFS_FAIL reason=not_found\n");
            return;
        }
    };
    match read_file(source, base_lba, &geometry, first_cluster, size) {
        Ok(contents) if slices_equal(&contents, CONTENT) => {
            debug_write("AW_MKFS_READBACK_OK\n");
            debug_write("AW_MKFS_PROOF_OK\n");
        }
        Ok(_) => debug_write("AW_MKFS_FAIL reason=content_mismatch\n"),
        Err(reason) => {
            debug_write("AW_MKFS_FAIL reason=read_");
            debug_write(reason);
            debug_write("\n");
        }
    }
}

/// Read a whole file by its 8.3 `name` from the FAT16 volume at `base_lba` on any
/// sector source, returning its bytes or [`None`] if the volume cannot be parsed or
/// the file is absent. The generic counterpart of [`load_file`], used by the disk
/// builder's full-stack read-back.
#[cfg(feature = "disk-build-smoke-test")]
pub fn read_named<S: SectorSource>(source: &S, base_lba: u64, name: &[u8; 11]) -> Option<Vec<u8>> {
    let mut boot = [0u8; SECTOR_SIZE];
    source.read_sector(base_lba, &mut boot).ok()?;
    let geometry = parse_geometry(&boot)?;
    let (first_cluster, size) = find_file(source, base_lba, &geometry, name).ok()??;
    read_file(source, base_lba, &geometry, first_cluster, size).ok()
}
