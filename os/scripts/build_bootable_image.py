#!/usr/bin/env python3
"""Build a bootable UEFI disk image (GPT + FAT16 EFI System Partition).

The QEMU proof harness boots the loader and kernel from QEMU's *virtual* FAT,
which never produces a standalone file. This builds the real thing: a GPT disk
image whose EFI System Partition is a FAT16 volume holding the staged tree
(``EFI/BOOT/BOOTX64.EFI`` and ``KERNEL.BIN``). UEFI firmware boots removable
media through ``\\EFI\\BOOT\\BOOTX64.EFI``, so the image boots in QEMU/OVMF and
can be written to a USB stick.

No external tools are used (none are available on the build host): the FAT16
filesystem and the GPT wrapper are assembled here from bytes.

Usage: build_bootable_image.py <output.img> <staged-esp-root-dir>
"""

import os
import struct
import sys
import zlib

SECTOR = 512
SECTORS_PER_CLUSTER = 4  # 2 KiB clusters
RESERVED_SECTORS = 1
NUM_FATS = 2
ROOT_ENTRIES = 512
# Cluster count kept comfortably inside the FAT16 range (4085..65524). At 2 KiB
# per cluster this is a ~56 MiB ESP - room for the loader and kernel plus the
# embedded speech (the accessible UEFI Setup and its command agent carry hundreds
# of fixed PCM clips plus the premium real-voice word bank for French and English),
# with headroom, and still small enough to write to USB quickly.
DATA_CLUSTERS = 28000

FAT16_EOC = 0xFFFF
# A fixed, valid FAT date/time (2026-01-01 00:00:00) so entries are not zero.
FIXED_DATE = ((2026 - 1980) << 9) | (1 << 5) | 1
FIXED_TIME = 0

ESP_TYPE_GUID = bytes(
    [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
     0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B]
)


class Dir:
    def __init__(self, name):
        self.name = name
        self.dirs = {}
        self.files = {}
        self.cluster = 0  # assigned later


def short_name(name):
    """Encode an 8.3 name into the 11-byte directory field."""
    if "." in name:
        base, ext = name.rsplit(".", 1)
    else:
        base, ext = name, ""
    base = base.upper()
    ext = ext.upper()
    if len(base) > 8 or len(ext) > 3:
        raise ValueError(f"name {name!r} does not fit 8.3")
    return base.ljust(8).encode("ascii") + ext.ljust(3).encode("ascii")


def build_tree(root_dir_path):
    root = Dir("")
    for dirpath, dirnames, filenames in os.walk(root_dir_path):
        rel = os.path.relpath(dirpath, root_dir_path)
        node = root
        if rel != ".":
            for part in rel.replace("\\", "/").split("/"):
                node = node.dirs[part]
        for d in dirnames:
            node.dirs[d] = Dir(d)
        for f in filenames:
            with open(os.path.join(dirpath, f), "rb") as handle:
                node.files[f] = handle.read()
    return root


def dir_entry(name11, attr, first_cluster, size):
    return struct.pack(
        "<11sBBBHHHHHHHI",
        name11,          # name
        attr,            # attributes
        0,               # NTRes
        0,               # CrtTimeTenth
        FIXED_TIME,      # CrtTime
        FIXED_DATE,      # CrtDate
        FIXED_DATE,      # LstAccDate
        0,               # FstClusHI (0 on FAT16)
        FIXED_TIME,      # WrtTime
        FIXED_DATE,      # WrtDate
        first_cluster,   # FstClusLO
        size,            # FileSize
    )


def build_fat16(root):
    cluster_size = SECTORS_PER_CLUSTER * SECTOR
    fat = [0] * (DATA_CLUSTERS + 2)
    fat[0] = 0xFFF8
    fat[1] = FAT16_EOC
    cluster_data = {}  # cluster number -> bytes (one cluster)
    next_cluster = [2]

    def alloc_chain(nclusters):
        clusters = list(range(next_cluster[0], next_cluster[0] + nclusters))
        next_cluster[0] += nclusters
        for i, c in enumerate(clusters):
            fat[c] = clusters[i + 1] if i + 1 < len(clusters) else FAT16_EOC
        return clusters

    def store(clusters, data):
        for i, c in enumerate(clusters):
            chunk = data[i * cluster_size:(i + 1) * cluster_size]
            cluster_data[c] = chunk.ljust(cluster_size, b"\x00")

    # First pass: assign a starting cluster to every subdirectory so that "."
    # and ".." entries can name real clusters.
    def assign_dirs(node):
        for child in node.dirs.values():
            child.cluster = alloc_chain(1)[0]
            assign_dirs(child)

    assign_dirs(root)

    # Second pass: files, then materialise directory contents.
    def build_dir_bytes(node, parent_cluster, is_root):
        entries = b""
        if not is_root:
            entries += dir_entry(b".          ", 0x10, node.cluster, 0)
            entries += dir_entry(b"..         ", 0x10, parent_cluster, 0)
        for name, sub in node.dirs.items():
            entries += dir_entry(short_name(name), 0x10, sub.cluster, 0)
        for name, data in node.files.items():
            nclusters = max(1, (len(data) + cluster_size - 1) // cluster_size)
            clusters = alloc_chain(nclusters)
            store(clusters, data)
            entries += dir_entry(short_name(name), 0x20, clusters[0], len(data))
        return entries

    # Subdirectories (each occupies its single assigned cluster here; a dir that
    # overflows one cluster is not needed for this tree).
    def fill_dirs(node, parent_cluster, is_root):
        # Recurse first so child file clusters are allocated before we emit the
        # parent's entries? Directory entries only need child *directory*
        # clusters (already assigned) and child file clusters (allocated when
        # this dir's bytes are built), so build this dir, then recurse.
        entries = build_dir_bytes(node, parent_cluster, is_root)
        if not is_root:
            if len(entries) > cluster_size:
                raise ValueError("directory too large for one cluster")
            cluster_data[node.cluster] = entries.ljust(cluster_size, b"\x00")
        for child in node.dirs.values():
            fill_dirs(child, node.cluster, False)
        return entries

    root_entries = fill_dirs(root, 0, True)

    used_clusters = next_cluster[0] - 2
    if used_clusters > DATA_CLUSTERS:
        raise ValueError("payload exceeds ESP capacity")

    # Geometry, consistent with the count-of-clusters the driver recomputes.
    fat_bytes = (DATA_CLUSTERS + 2) * 2
    fat_sectors = (fat_bytes + SECTOR - 1) // SECTOR
    root_dir_sectors = (ROOT_ENTRIES * 32 + SECTOR - 1) // SECTOR
    data_sectors = DATA_CLUSTERS * SECTORS_PER_CLUSTER
    total_sectors = (
        RESERVED_SECTORS + NUM_FATS * fat_sectors + root_dir_sectors + data_sectors
    )

    image = bytearray(total_sectors * SECTOR)

    # Boot sector / BPB (FAT16).
    bs = bytearray(SECTOR)
    bs[0:3] = b"\xEB\x3C\x90"
    bs[3:11] = b"MSWIN4.1"
    struct.pack_into(
        "<HBHBHHBHHHII",
        bs, 11,
        SECTOR,               # BPB_BytsPerSec
        SECTORS_PER_CLUSTER,  # BPB_SecPerClus
        RESERVED_SECTORS,     # BPB_RsvdSecCnt
        NUM_FATS,             # BPB_NumFATs
        ROOT_ENTRIES,         # BPB_RootEntCnt
        0,                    # BPB_TotSec16 (0 -> use TotSec32)
        0xF8,                 # BPB_Media
        fat_sectors,          # BPB_FATSz16
        32,                   # BPB_SecPerTrk
        8,                    # BPB_NumHeads
        0,                    # BPB_HiddSec (partition start filled by caller)
        total_sectors,        # BPB_TotSec32
    )
    bs[36] = 0x80             # BS_DrvNum
    bs[37] = 0x00             # reserved
    bs[38] = 0x29             # BS_BootSig (extended fields present)
    struct.pack_into("<I", bs, 39, 0xA1CE_B007)  # BS_VolID
    bs[43:54] = b"AWIN BOOT  "                     # BS_VolLab
    bs[54:62] = b"FAT16   "                        # BS_FilSysType
    bs[510:512] = b"\x55\xAA"
    image[0:SECTOR] = bs

    # FATs.
    fat_region = bytearray(fat_sectors * SECTOR)
    for i, value in enumerate(fat):
        struct.pack_into("<H", fat_region, i * 2, value & 0xFFFF)
    for n in range(NUM_FATS):
        off = (RESERVED_SECTORS + n * fat_sectors) * SECTOR
        image[off:off + len(fat_region)] = fat_region

    # Root directory region.
    root_off = (RESERVED_SECTORS + NUM_FATS * fat_sectors) * SECTOR
    image[root_off:root_off + len(root_entries)] = root_entries

    # Data region.
    data_start = RESERVED_SECTORS + NUM_FATS * fat_sectors + root_dir_sectors
    for cluster, chunk in cluster_data.items():
        off = (data_start + (cluster - 2) * SECTORS_PER_CLUSTER) * SECTOR
        image[off:off + len(chunk)] = chunk

    return bytes(image), total_sectors


def crc32(data):
    return zlib.crc32(data) & 0xFFFFFFFF


def build_gpt(fat_image, fat_sectors):
    part_start = 2048  # 1 MiB alignment
    part_end = part_start + fat_sectors - 1
    # Backup GPT: 32-sector entry array + 1 header at the very end.
    total = part_end + 1 + 32 + 1
    last_usable = total - 34
    if last_usable < part_end:
        total = part_end + 34
        last_usable = total - 34

    disk = bytearray(total * SECTOR)

    # Protective MBR.
    mbr = bytearray(SECTOR)
    struct.pack_into(
        "<BBBBBBBBII", mbr, 446,
        0x00, 0x00, 0x02, 0x00, 0xEE, 0xFF, 0xFF, 0xFF,
        1, min(total - 1, 0xFFFFFFFF),
    )
    mbr[510:512] = b"\x55\xAA"
    disk[0:SECTOR] = mbr

    # Partition entry array (128 entries * 128 bytes = 32 sectors).
    entries = bytearray(128 * 128)
    entry = ESP_TYPE_GUID
    entry += os.urandom(16)                       # unique partition GUID
    entry += struct.pack("<QQ", part_start, part_end)
    entry += struct.pack("<Q", 0)                 # attributes
    entry += "EFI System Partition".encode("utf-16-le").ljust(72, b"\x00")
    entries[0:128] = entry
    array_crc = crc32(entries)

    disk_guid = os.urandom(16)

    def header(my_lba, alt_lba, entries_lba):
        h = bytearray(92)
        h[0:8] = b"EFI PART"
        struct.pack_into("<III", h, 8, 0x00010000, 92, 0)  # rev, size, crc=0
        struct.pack_into("<QQQQ", h, 24, my_lba, alt_lba, 34, last_usable)
        h[56:72] = disk_guid
        struct.pack_into("<QII", h, 72, entries_lba, 128, 128)
        struct.pack_into("<I", h, 88, array_crc)
        struct.pack_into("<I", h, 16, crc32(bytes(h)))
        return bytes(h)

    # Primary header at LBA 1, entries at LBA 2.
    disk[1 * SECTOR:1 * SECTOR + 92] = header(1, total - 1, 2)
    disk[2 * SECTOR:2 * SECTOR + len(entries)] = entries

    # Backup entries and header at the end.
    backup_entries_lba = total - 33
    disk[backup_entries_lba * SECTOR:backup_entries_lba * SECTOR + len(entries)] = entries
    disk[(total - 1) * SECTOR:(total - 1) * SECTOR + 92] = header(
        total - 1, 1, backup_entries_lba
    )

    # The FAT partition, with BPB_HiddSec set to its start LBA.
    fat = bytearray(fat_image)
    struct.pack_into("<I", fat, 28, part_start)  # BPB_HiddSec
    disk[part_start * SECTOR:part_start * SECTOR + len(fat)] = fat

    return bytes(disk)


def main():
    # `--fat-only OUT DIR` writes a bare FAT16 filesystem image (no GPT), used as
    # a data disk. Otherwise `OUT DIR` writes a bootable GPT + ESP disk.
    args = sys.argv[1:]
    fat_only = False
    if args and args[0] == "--fat-only":
        fat_only = True
        args = args[1:]
    if len(args) != 2:
        print(__doc__)
        return 2
    out_path, esp_root = args
    root = build_tree(esp_root)
    fat_image, fat_sectors = build_fat16(root)
    if fat_only:
        with open(out_path, "wb") as handle:
            handle.write(fat_image)
        print(f"wrote {out_path}: {len(fat_image)} bytes, {fat_sectors} sectors (bare FAT16)")
        return 0
    disk = build_gpt(fat_image, fat_sectors)
    with open(out_path, "wb") as handle:
        handle.write(disk)
    print(f"wrote {out_path}: {len(disk)} bytes, {len(disk)//SECTOR} sectors "
          f"(ESP {fat_sectors} sectors)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
