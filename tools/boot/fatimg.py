#!/usr/bin/env python3
"""Minimal FAT16 volume images, standard library only: build an ESP, read files back.

UEFI boots a FAT volume written directly to a disk (no partition table), so a test ESP is
just this image. Unlike QEMU's virtual FAT folder (vvfat), whose write support asserts under
directory updates, a real image makes the loader's own writes (boot-state records,
diagnostics) persistent and reliable across boots - and readable afterwards by `read`.

    fatimg.py build IMAGE SIZE_MIB DEST=SOURCE [DEST=SOURCE ...]   e.g. EFI/BOOT/BOOTX64.EFI=loader.efi
    fatimg.py read  IMAGE PATH [OUTPUT]                            prints size, or writes the file
    fatimg.py put   IMAGE PATH SOURCE                              replace/add one file

Names are 8.3 (upper case); that is all the firmware ESP layout needs.
"""
from __future__ import annotations

import struct
import sys
from pathlib import Path

SECTOR = 512
SECTORS_PER_CLUSTER = 4          # 2 KiB clusters
RESERVED = 4
FATS = 2
ROOT_ENTRIES = 512
ATTR_DIR, ATTR_ARCHIVE = 0x10, 0x20
EOC = 0xFFFF


def short_name(name: str) -> bytes:
    base, _, ext = name.upper().partition(".")
    if not base or len(base) > 8 or len(ext) > 3:
        raise ValueError(f"not an 8.3 name: {name}")
    return base.ljust(8).encode("ascii") + ext.ljust(3).encode("ascii")


class Volume:
    def __init__(self, data: bytearray):
        self.data = data
        (bps, spc, reserved, fats, root_entries, total16, _media, fat_sectors, _spt, _heads,
         _hidden, total32) = struct.unpack_from("<HBHBHHBHHHII", data, 11)
        assert bps == SECTOR, "unsupported sector size"
        self.spc = spc
        self.fat_offset = reserved * SECTOR
        self.fat_bytes = fat_sectors * SECTOR
        self.fats = fats
        self.root_offset = self.fat_offset + fats * self.fat_bytes
        self.root_entries = root_entries
        self.data_offset = self.root_offset + root_entries * 32
        self.cluster_bytes = spc * SECTOR
        total = total16 or total32
        self.clusters = (total * SECTOR - self.data_offset) // self.cluster_bytes

    # -- FAT --------------------------------------------------------------------------------
    def fat(self, cluster: int) -> int:
        return struct.unpack_from("<H", self.data, self.fat_offset + 2 * cluster)[0]

    def set_fat(self, cluster: int, value: int) -> None:
        for copy in range(self.fats):
            struct.pack_into("<H", self.data, self.fat_offset + copy * self.fat_bytes + 2 * cluster, value)

    def chain(self, first: int) -> list[int]:
        out = []
        while 2 <= first < 0xFFF8 and len(out) <= self.clusters:
            out.append(first)
            first = self.fat(first)
        return out

    def allocate(self, count: int) -> list[int]:
        free = [c for c in range(2, self.clusters + 2) if self.fat(c) == 0][:count]
        if len(free) < count:
            raise OSError("volume full")
        for a, b in zip(free, free[1:] + [None]):
            self.set_fat(a, EOC if b is None else b)
        return free

    def cluster_offset(self, cluster: int) -> int:
        return self.data_offset + (cluster - 2) * self.cluster_bytes

    # -- directories ------------------------------------------------------------------------
    def entries(self, cluster: int | None):
        """(offset, entry bytes) of every slot in a directory (None = root)."""
        if cluster is None:
            spans = [(self.root_offset, self.root_entries * 32)]
        else:
            spans = [(self.cluster_offset(c), self.cluster_bytes) for c in self.chain(cluster)]
        for base, length in spans:
            for off in range(base, base + length, 32):
                yield off, bytes(self.data[off:off + 32])

    def find(self, directory: int | None, name: str):
        want = short_name(name)
        for off, entry in self.entries(directory):
            if entry[0] == 0:
                return None
            if entry[0] != 0xE5 and entry[11] != 0x0F and entry[:11] == want:
                return off, entry
        return None

    def free_slot(self, directory: int | None) -> int:
        for off, entry in self.entries(directory):
            if entry[0] in (0, 0xE5):
                return off
        if directory is None:
            raise OSError("root directory full")
        last = self.chain(directory)[-1]
        (new,) = self.allocate(1)
        self.set_fat(last, new)
        self.data[self.cluster_offset(new):self.cluster_offset(new) + self.cluster_bytes] = bytes(self.cluster_bytes)
        return self.cluster_offset(new)

    def write_entry(self, off: int, name: bytes, attr: int, cluster: int, size: int) -> None:
        entry = bytearray(32)
        entry[:11] = name
        entry[11] = attr
        struct.pack_into("<HH", entry, 20, cluster >> 16, 0)
        struct.pack_into("<HHHI", entry, 22, 0, 0x5B3C, cluster & 0xFFFF, size)  # fixed date
        self.data[off:off + 32] = entry

    def mkdir(self, parent: int | None, name: str) -> int:
        found = self.find(parent, name)
        if found:
            return struct.unpack_from("<H", found[1], 26)[0]
        (cluster,) = self.allocate(1)
        base = self.cluster_offset(cluster)
        self.data[base:base + self.cluster_bytes] = bytes(self.cluster_bytes)
        self.write_entry(base, b".          ", ATTR_DIR, cluster, 0)
        self.write_entry(base + 32, b"..         ", ATTR_DIR, parent or 0, 0)
        self.write_entry(self.free_slot(parent), short_name(name), ATTR_DIR, cluster, 0)
        return cluster

    def walk(self, path: str, create: bool):
        """(exists, directory cluster or None for the root, final name)."""
        parts = [p for p in path.replace("\\", "/").split("/") if p]
        directory = None
        for part in parts[:-1]:
            if create:
                directory = self.mkdir(directory, part)
            else:
                found = self.find(directory, part)
                if not found or not found[1][11] & ATTR_DIR:
                    return False, None, parts[-1]
                directory = struct.unpack_from("<H", found[1], 26)[0]
        return True, directory, parts[-1]

    def put(self, path: str, content: bytes) -> None:
        _, directory, name = self.walk(path, create=True)
        found = self.find(directory, name)
        if found:
            for c in self.chain(struct.unpack_from("<H", found[1], 26)[0]):
                self.set_fat(c, 0)
            off = found[0]
        else:
            off = self.free_slot(directory)
        clusters = self.allocate(max(1, -(-len(content) // self.cluster_bytes))) if content else []
        for i, c in enumerate(clusters):
            chunk = content[i * self.cluster_bytes:(i + 1) * self.cluster_bytes]
            base = self.cluster_offset(c)
            self.data[base:base + self.cluster_bytes] = chunk.ljust(self.cluster_bytes, b"\0")
        self.write_entry(off, short_name(name), ATTR_ARCHIVE, clusters[0] if clusters else 0, len(content))

    def get(self, path: str) -> bytes | None:
        exists, directory, name = self.walk(path, create=False)
        if not exists:
            return None
        found = self.find(directory, name)
        if not found or found[1][11] & ATTR_DIR:
            return None
        cluster = struct.unpack_from("<H", found[1], 26)[0]
        size = struct.unpack_from("<I", found[1], 28)[0]
        blob = b"".join(bytes(self.data[self.cluster_offset(c):self.cluster_offset(c) + self.cluster_bytes])
                        for c in self.chain(cluster))
        return blob[:size]


def format_volume(size_mib: int) -> bytearray:
    total = size_mib * 1024 * 1024 // SECTOR
    clusters_guess = total // SECTORS_PER_CLUSTER
    fat_sectors = -(-(clusters_guess + 2) * 2 // SECTOR)
    data = bytearray(total * SECTOR)
    boot = bytearray(SECTOR)
    boot[0:3] = b"\xEB\x3C\x90"
    boot[3:11] = b"OMNIOS  "
    total16, total32 = (total, 0) if total < 0x10000 else (0, total)
    struct.pack_into("<HBHBHHBHHHII", boot, 11, SECTOR, SECTORS_PER_CLUSTER, RESERVED, FATS,
                     ROOT_ENTRIES, total16, 0xF8, fat_sectors, 32, 64, 0, total32)
    struct.pack_into("<BBBI11s8s", boot, 36, 0x80, 0, 0x29, 0x4F4D4E49, b"OMNI-OS ESP", b"FAT16   ")
    boot[510:512] = b"\x55\xAA"
    data[0:SECTOR] = boot
    volume = Volume(data)
    for copy in range(FATS):
        struct.pack_into("<HH", data, volume.fat_offset + copy * volume.fat_bytes, 0xFFF8, 0xFFFF)
    clusters = volume.clusters
    assert 4085 <= clusters < 65525, f"{clusters} clusters is not FAT16"
    return data


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    command, image = argv[1], Path(argv[2])
    if command == "build":
        data = format_volume(int(argv[3]))
        volume = Volume(data)
        for spec in argv[4:]:
            dest, _, source = spec.partition("=")
            volume.put(dest, Path(source).read_bytes())
        image.write_bytes(bytes(data))
        return 0
    data = bytearray(image.read_bytes())
    volume = Volume(data)
    if command == "read":
        content = volume.get(argv[3])
        if content is None:
            print(f"absent: {argv[3]}", file=sys.stderr)
            return 1
        if len(argv) > 4:
            Path(argv[4]).write_bytes(content)
        else:
            print(len(content))
        return 0
    if command == "put":
        volume.put(argv[3], Path(argv[4]).read_bytes())
        image.write_bytes(bytes(data))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
