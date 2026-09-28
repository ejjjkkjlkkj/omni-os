#!/usr/bin/env python3
from __future__ import annotations
import hashlib
import struct
import sys
from pathlib import Path

RELATION = bytes.fromhex("00ffff00")
EXCHANGE = bytes.fromhex("ff0000ff")
PASS_FRAME = (
    b"QEVARYNOX-A11Y-BOOT-V1\r\n"
    b"STATE=BOOT_CORE\r\n"
    b"STATUS=READY\r\n"
    b"CHANNEL=NONVISUAL_SERIAL\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"ACTION_REQUIRED=0\r\n"
    b"SEMANTIC_SOURCE=NATIVE_RELATION_GATE\r\n"
    b"END\r\n"
)
FAIL_FRAME = (
    b"QEVARYNOX-A11Y-BOOT-V1\r\n"
    b"STATE=BOOT_CORE\r\n"
    b"STATUS=BLOCKED\r\n"
    b"REASON=NATIVE_RELATION_INVALID\r\n"
    b"CHANNEL=NONVISUAL_SERIAL\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"ACTION_REQUIRED=0\r\n"
    b"SEMANTIC_SOURCE=NATIVE_RELATION_GATE\r\n"
    b"END\r\n"
)

class Code:
    def __init__(self):
        self.data = bytearray()
        self.labels = {}
        self.fixups = []

    def pos(self):
        return len(self.data)

    def emit(self, data):
        self.data += bytes(data)

    def label(self, name):
        self.labels[name] = self.pos()

    def rel32(self, opcode, label):
        self.emit(opcode)
        off = self.pos()
        self.emit(b"\x00" * 4)
        self.fixups.append((off, self.pos(), label, 4))

    def rel8(self, opcode, label):
        self.emit(bytes((opcode, 0)))
        self.fixups.append((self.pos() - 1, self.pos(), label, 1))

    def lea_r10(self, label):
        self.emit(b"\x4c\x8d\x15")
        off = self.pos()
        self.emit(b"\x00" * 4)
        self.fixups.append((off, self.pos(), label, 4))

    def patch(self):
        for off, after, label, width in self.fixups:
            disp = self.labels[label] - after
            if width == 4:
                struct.pack_into("<i", self.data, off, disp)
            else:
                if not -128 <= disp <= 127:
                    raise SystemExit("short branch overflow")
                self.data[off] = disp & 0xff

def build_code():
    c = Code()
    c.lea_r10("relation")

    def mov_al(d):
        c.emit(b"\x41\x8a\x02" if d == 0 else bytes((0x41, 0x8a, 0x42, d)))

    def cmp_al(d):
        c.emit(bytes((0x41, 0x3a, 0x42, d)))

    def jne_fail():
        c.rel32(b"\x0f\x85", "fail")

    def je_fail():
        c.rel32(b"\x0f\x84", "fail")

    mov_al(0); cmp_al(1); je_fail()
    mov_al(1); cmp_al(2); jne_fail()
    mov_al(0); cmp_al(3); jne_fail()
    mov_al(4); cmp_al(5); je_fail()
    mov_al(5); cmp_al(6); jne_fail()
    mov_al(4); cmp_al(7); jne_fail()
    mov_al(0); cmp_al(5); jne_fail()
    mov_al(1); cmp_al(4); jne_fail()

    c.lea_r10("pass")
    c.emit(b"\xb9" + struct.pack("<I", len(PASS_FRAME)))
    c.rel32(b"\xe9", "emit")

    c.label("fail")
    c.lea_r10("failmsg")
    c.emit(b"\xb9" + struct.pack("<I", len(FAIL_FRAME)))

    c.label("emit")
    c.emit(b"\x66\xba\xfd\x03")
    c.label("wait")
    c.emit(b"\xec\xa8\x20")
    c.rel8(0x74, "wait")
    c.emit(b"\x66\xba\xf8\x03")
    c.emit(b"\x41\x8a\x02")
    c.emit(b"\xee")
    c.emit(b"\x49\xff\xc2")
    c.emit(b"\x66\xba\xfd\x03")
    c.emit(b"\xff\xc9")
    c.rel8(0x75, "wait")
    c.label("halt")
    c.emit(b"\xf4")
    c.rel8(0xeb, "halt")

    c.label("relation")
    c.emit(RELATION + EXCHANGE)
    c.label("pass")
    c.emit(PASS_FRAME)
    c.label("failmsg")
    c.emit(FAIL_FRAME)
    c.patch()
    return bytes(c.data), c.labels

def put(buf, off, fmt, *values):
    struct.pack_into(fmt, buf, off, *values)

def build_image():
    code, labels = build_code()
    image = bytearray(0x800)
    put(image, 0x00, "<H", 0x5A4D)
    put(image, 0x3C, "<I", 0x80)
    pe = 0x80
    image[pe:pe+4] = b"PE\0\0"
    coff = pe + 4
    put(image, coff, "<HHIIIHH", 0x8664, 2, 0, 0, 0, 0xF0, 0x0022)
    opt = coff + 20
    put(image, opt + 0x00, "<H", 0x20B)
    put(image, opt + 0x04, "<I", 0x400)
    put(image, opt + 0x08, "<I", 0x200)
    put(image, opt + 0x10, "<I", 0x1000)
    put(image, opt + 0x14, "<I", 0x1000)
    put(image, opt + 0x18, "<Q", 0x400000)
    put(image, opt + 0x20, "<I", 0x1000)
    put(image, opt + 0x24, "<I", 0x200)
    put(image, opt + 0x38, "<I", 0x3000)
    put(image, opt + 0x3C, "<I", 0x200)
    put(image, opt + 0x44, "<H", 10)
    put(image, opt + 0x48, "<Q", 0x100000)
    put(image, opt + 0x50, "<Q", 0x1000)
    put(image, opt + 0x58, "<Q", 0x100000)
    put(image, opt + 0x60, "<Q", 0x1000)
    put(image, opt + 0x6C, "<I", 16)
    put(image, opt + 0x70 + 5 * 8, "<II", 0x2000, 8)

    sec = opt + 0xF0
    image[sec:sec+8] = b".text\0\0\0"
    put(image, sec + 0x08, "<I", len(code))
    put(image, sec + 0x0C, "<I", 0x1000)
    put(image, sec + 0x10, "<I", 0x400)
    put(image, sec + 0x14, "<I", 0x200)
    put(image, sec + 0x24, "<I", 0x60000020)

    reloc = sec + 40
    image[reloc:reloc+8] = b".reloc\0\0"
    put(image, reloc + 0x08, "<I", 8)
    put(image, reloc + 0x0C, "<I", 0x2000)
    put(image, reloc + 0x10, "<I", 0x200)
    put(image, reloc + 0x14, "<I", 0x600)
    put(image, reloc + 0x24, "<I", 0x42000040)

    image[0x200:0x200+len(code)] = code
    put(image, 0x600, "<II", 0x1000, 8)
    return bytes(image), labels

def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_native_access.py OUTPUT")
    image, labels = build_image()
    if len(image) != 2048:
        raise SystemExit("unexpected image size")
    if 0x200 + labels["relation"] != 0x2B0:
        raise SystemExit("relation offset changed")
    if image[0x2B0:0x2B4] != RELATION or image[0x2B4:0x2B8] != EXCHANGE:
        raise SystemExit("native relation binding mismatch")
    digest = hashlib.sha256(image).hexdigest()
    if digest != "f2f3d74a20d52e250d4a41c25a10a60f664a65bcfb6e000546655711e2e30473":
        raise SystemExit("deterministic image digest mismatch: " + digest)
    out = Path(sys.argv[1])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print("OS_NATIVE_ACCESS_BOOT_BUILD=PASS")
    print("bytes=2048")
    print("relation-offset=0x2b0")
    print("sha256=" + digest)

if __name__ == "__main__":
    main()
