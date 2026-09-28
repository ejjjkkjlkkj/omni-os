#!/usr/bin/env python3
from __future__ import annotations
import hashlib
import struct
import sys
from pathlib import Path

MARKER = b"QEVARYNOX-UEFI-SEED-V1-PASS\r\n"
FILE_ALIGNMENT = 0x200
SECTION_ALIGNMENT = 0x1000
TEXT_RVA = 0x1000
RELOC_RVA = 0x2000
HEADERS_SIZE = 0x200
TEXT_RAW = 0x200
RELOC_RAW = 0x400
FILE_SIZE = 0x600

def put(buf: bytearray, off: int, fmt: str, *values: int) -> None:
    struct.pack_into(fmt, buf, off, *values)

def build_code() -> bytes:
    # Direct nonvisual COM1 witness. No firmware console protocol, runtime,
    # assembler, compiler, linker, UIA, or screen reader is involved.
    code = bytearray()
    data_offset = 43
    code += b"\x4c\x8d\x15" + struct.pack("<i", data_offset - 7)  # lea r10,[rip+marker]
    code += b"\xb9" + struct.pack("<I", len(MARKER))               # mov ecx,len
    code += b"\x66\xba\xfd\x03"                                 # dx=COM1 LSR
    code += b"\xec\xa8\x20\x74\xfb"                             # wait THR empty
    code += b"\x66\xba\xf8\x03"                                 # dx=COM1 THR
    code += b"\x41\x8a\x02\xee"                                 # al=[r10]; out dx,al
    code += b"\x49\xff\xc2"                                      # inc r10
    code += b"\x66\xba\xfd\x03"                                 # dx=COM1 LSR
    code += b"\xff\xc9\x75\xe8"                                 # dec ecx; loop
    code += b"\xf4\xeb\xfd"                                      # halt loop
    if len(code) != data_offset:
        raise SystemExit("seed code layout mismatch")
    code += MARKER
    return bytes(code)

def build_image() -> bytes:
    code = build_code()
    if len(code) > FILE_ALIGNMENT:
        raise SystemExit("seed code exceeds one raw section")

    image = bytearray(FILE_SIZE)
    # DOS compatibility header: only MZ + e_lfanew are semantically needed.
    put(image, 0x00, "<H", 0x5A4D)
    put(image, 0x3C, "<I", 0x80)

    pe = 0x80
    image[pe:pe+4] = b"PE\0\0"
    coff = pe + 4
    put(image, coff, "<HHIIIHH",
        0x8664,     # AMD64
        2,          # .text + .reloc
        0, 0, 0,
        0xF0,       # PE32+ optional header bytes
        0x0022)     # executable + large-address-aware

    opt = coff + 20
    put(image, opt + 0x00, "<H", 0x20B)               # PE32+
    put(image, opt + 0x04, "<I", FILE_ALIGNMENT)      # SizeOfCode
    put(image, opt + 0x08, "<I", FILE_ALIGNMENT)      # SizeOfInitializedData
    put(image, opt + 0x10, "<I", TEXT_RVA)            # AddressOfEntryPoint
    put(image, opt + 0x14, "<I", TEXT_RVA)            # BaseOfCode
    put(image, opt + 0x18, "<Q", 0x400000)            # ImageBase
    put(image, opt + 0x20, "<I", SECTION_ALIGNMENT)
    put(image, opt + 0x24, "<I", FILE_ALIGNMENT)
    put(image, opt + 0x38, "<I", 0x3000)              # SizeOfImage
    put(image, opt + 0x3C, "<I", HEADERS_SIZE)
    put(image, opt + 0x44, "<H", 10)                  # EFI application subsystem
    put(image, opt + 0x48, "<Q", 0x100000)            # stack reserve
    put(image, opt + 0x50, "<Q", 0x1000)              # stack commit
    put(image, opt + 0x58, "<Q", 0x100000)            # heap reserve
    put(image, opt + 0x60, "<Q", 0x1000)              # heap commit
    put(image, opt + 0x6C, "<I", 16)                  # data-directory count
    # Base relocation data directory (index 5).
    put(image, opt + 0x70 + (5 * 8), "<II", RELOC_RVA, 12)

    sec = opt + 0xF0
    image[sec:sec+8] = b".text\0\0\0"
    put(image, sec + 0x08, "<I", len(code))
    put(image, sec + 0x0C, "<I", TEXT_RVA)
    put(image, sec + 0x10, "<I", FILE_ALIGNMENT)
    put(image, sec + 0x14, "<I", TEXT_RAW)
    put(image, sec + 0x24, "<I", 0x60000020)           # code + execute + read

    reloc = sec + 40
    image[reloc:reloc+8] = b".reloc\0\0"
    put(image, reloc + 0x08, "<I", 12)
    put(image, reloc + 0x0C, "<I", RELOC_RVA)
    put(image, reloc + 0x10, "<I", FILE_ALIGNMENT)
    put(image, reloc + 0x14, "<I", RELOC_RAW)
    put(image, reloc + 0x24, "<I", 0x42000040)         # initialized + discardable + read

    image[TEXT_RAW:TEXT_RAW+len(code)] = code
    # One relocation block with only ABSOLUTE/no-op entries. The seed has no
    # absolute addresses; the directory exists so the firmware may relocate it.
    put(image, RELOC_RAW, "<IIHH", TEXT_RVA, 12, 0, 0)
    return bytes(image)

def validate(image: bytes) -> None:
    if len(image) != FILE_SIZE:
        raise SystemExit("unexpected image size")
    if image[:2] != b"MZ":
        raise SystemExit("missing DOS signature")
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[pe:pe+4] != b"PE\0\0":
        raise SystemExit("missing PE signature")
    coff = pe + 4
    machine, sections = struct.unpack_from("<HH", image, coff)
    if machine != 0x8664 or sections != 2:
        raise SystemExit("unexpected PE machine/section count")
    opt = coff + 20
    if struct.unpack_from("<H", image, opt)[0] != 0x20B:
        raise SystemExit("not PE32+")
    if struct.unpack_from("<I", image, opt + 0x10)[0] != TEXT_RVA:
        raise SystemExit("unexpected entrypoint")
    if struct.unpack_from("<H", image, opt + 0x44)[0] != 10:
        raise SystemExit("not EFI application subsystem")

def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_seed.py OUTPUT")
    out = Path(sys.argv[1])
    image = build_image()
    validate(image)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print("UEFI_SEED_BUILD=PASS")
    print(f"bytes={len(image)}")
    print(f"marker={MARKER.decode('ascii').strip()}")
    print(f"sha256={hashlib.sha256(image).hexdigest()}")

if __name__ == "__main__":
    main()
