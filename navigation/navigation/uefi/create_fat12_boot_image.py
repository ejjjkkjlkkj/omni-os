#!/usr/bin/env python3
from __future__ import annotations

import argparse
import math
import struct
from pathlib import Path

SECTOR = 512
TOTAL_SECTORS = 2880
RESERVED = 1
FATS = 2
SECTORS_PER_FAT = 9
ROOT_ENTRIES = 224
ROOT_SECTORS = (ROOT_ENTRIES * 32 + SECTOR - 1) // SECTOR
DATA_START_SECTOR = RESERVED + FATS * SECTORS_PER_FAT + ROOT_SECTORS
DATA_CLUSTERS = TOTAL_SECTORS - DATA_START_SECTOR


def set_fat12(fat: bytearray, cluster: int, value: int) -> None:
    value &= 0xFFF
    off = cluster + cluster // 2
    if cluster & 1:
        fat[off] = (fat[off] & 0x0F) | ((value << 4) & 0xF0)
        fat[off + 1] = (value >> 4) & 0xFF
    else:
        fat[off] = value & 0xFF
        fat[off + 1] = (fat[off + 1] & 0xF0) | ((value >> 8) & 0x0F)


def dir_entry(name11: bytes, attr: int, cluster: int, size: int = 0) -> bytes:
    if len(name11) != 11:
        raise ValueError("8.3 name must be exactly 11 bytes")
    e = bytearray(32)
    e[0:11] = name11
    e[11] = attr
    struct.pack_into("<H", e, 26, cluster)
    struct.pack_into("<I", e, 28, size)
    return bytes(e)


def cluster_offset(cluster: int) -> int:
    return (DATA_START_SECTOR + (cluster - 2)) * SECTOR


def make_image(efi_path: Path, output: Path) -> None:
    payload = efi_path.read_bytes()
    file_clusters = max(1, math.ceil(len(payload) / SECTOR))
    first_file_cluster = 4
    last_file_cluster = first_file_cluster + file_clusters - 1
    if last_file_cluster >= DATA_CLUSTERS + 2:
        raise SystemExit("EFI payload does not fit in FAT12 boot image")

    image = bytearray(TOTAL_SECTORS * SECTOR)
    bs = memoryview(image)[:SECTOR]
    bs[0:3] = b"\xEB\x3C\x90"
    bs[3:11] = b"MSDOS5.0"
    struct.pack_into("<H", bs, 11, SECTOR)
    bs[13] = 1
    struct.pack_into("<H", bs, 14, RESERVED)
    bs[16] = FATS
    struct.pack_into("<H", bs, 17, ROOT_ENTRIES)
    struct.pack_into("<H", bs, 19, TOTAL_SECTORS)
    bs[21] = 0xF0
    struct.pack_into("<H", bs, 22, SECTORS_PER_FAT)
    struct.pack_into("<H", bs, 24, 18)
    struct.pack_into("<H", bs, 26, 2)
    struct.pack_into("<I", bs, 28, 0)
    struct.pack_into("<I", bs, 32, 0)
    bs[36] = 0
    bs[37] = 0
    bs[38] = 0x29
    struct.pack_into("<I", bs, 39, 0x51564546)
    bs[43:54] = b"QEVARYNOX  "
    bs[54:62] = b"FAT12   "
    bs[510:512] = b"\x55\xAA"

    fat = bytearray(SECTORS_PER_FAT * SECTOR)
    fat[0:3] = b"\xF0\xFF\xFF"
    set_fat12(fat, 2, 0xFFF)
    set_fat12(fat, 3, 0xFFF)
    for c in range(first_file_cluster, last_file_cluster + 1):
        set_fat12(fat, c, 0xFFF if c == last_file_cluster else c + 1)

    fat1 = RESERVED * SECTOR
    fat2 = (RESERVED + SECTORS_PER_FAT) * SECTOR
    image[fat1:fat1 + len(fat)] = fat
    image[fat2:fat2 + len(fat)] = fat

    root = (RESERVED + FATS * SECTORS_PER_FAT) * SECTOR
    image[root:root + 32] = dir_entry(b"EFI        ", 0x10, 2)

    efi_dir = cluster_offset(2)
    image[efi_dir:efi_dir + 32] = dir_entry(b".          ", 0x10, 2)
    image[efi_dir + 32:efi_dir + 64] = dir_entry(b"..         ", 0x10, 0)
    image[efi_dir + 64:efi_dir + 96] = dir_entry(b"BOOT       ", 0x10, 3)

    boot_dir = cluster_offset(3)
    image[boot_dir:boot_dir + 32] = dir_entry(b".          ", 0x10, 3)
    image[boot_dir + 32:boot_dir + 64] = dir_entry(b"..         ", 0x10, 2)
    image[boot_dir + 64:boot_dir + 96] = dir_entry(
        b"BOOTX64 EFI", 0x20, first_file_cluster, len(payload)
    )

    for idx in range(file_clusters):
        cluster = first_file_cluster + idx
        chunk = payload[idx * SECTOR:(idx + 1) * SECTOR]
        off = cluster_offset(cluster)
        image[off:off + len(chunk)] = chunk

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(image)
    print("FAT12_UEFI_IMAGE=PASS")
    print(f"EFI_BYTES={len(payload)}")
    print(f"IMAGE_BYTES={len(image)}")
    print("BOOT_PATH=EFI/BOOT/BOOTX64.EFI")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("efi", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()
    make_image(args.efi, args.output)


if __name__ == "__main__":
    main()
