#!/usr/bin/env python3
from __future__ import annotations
import hashlib
import struct
import sys
from pathlib import Path

TEXT_RVA = 0x1000
DATA_RVA = 0x2000
RELOC_RVA = 0x3000

MENU = (
    b"QEVARYNOX-A11Y-UEFI-MENU-V1\r\n"
    b"STATE=UEFI_NATIVE_MENU\r\n"
    b"ITEM=1;ID=CONTINUE;LABEL=Continue boot\r\n"
    b"ITEM=2;ID=ACCESSIBILITY;LABEL=Accessibility help\r\n"
    b"ITEM=3;ID=RECOVERY;LABEL=Recovery status\r\n"
    b"PROMPT=PRESS_1_2_3\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"KEYBOARD_REQUIRED=1\r\n"
    b"CHANNEL=NATIVE_COM1\r\n"
    b"SEMANTIC_SOURCE=UEFI_NATIVE_MENU_STATE\r\n"
    b"END\r\n"
)
HELP = (
    b"QEVARYNOX-A11Y-UEFI-MENU-V1\r\n"
    b"EVENT=SELECT\r\n"
    b"ID=ACCESSIBILITY\r\n"
    b"STATUS=READY\r\n"
    b"HELP=Press 1 to continue boot. Press 2 for accessibility help. Press 3 for recovery status.\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"END\r\n"
)
RECOVERY = (
    b"QEVARYNOX-A11Y-UEFI-MENU-V1\r\n"
    b"EVENT=SELECT\r\n"
    b"ID=RECOVERY\r\n"
    b"STATUS=BLOCKED\r\n"
    b"REASON=RECOVERY_NOT_ESTABLISHED\r\n"
    b"ACTION_REQUIRED=0\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"END\r\n"
)
INVALID = (
    b"QEVARYNOX-A11Y-UEFI-MENU-V1\r\n"
    b"EVENT=INVALID_KEY\r\n"
    b"STATUS=BLOCKED\r\n"
    b"REASON=KEY_NOT_MAPPED\r\n"
    b"PROMPT=PRESS_1_2_3\r\n"
    b"END\r\n"
)
CONTINUE = (
    b"QEVARYNOX-A11Y-UEFI-MENU-V1\r\n"
    b"EVENT=SELECT\r\n"
    b"ID=CONTINUE\r\n"
    b"STATUS=CONFIRMED\r\n"
    b"ACTION=RETURN_TO_FIRMWARE_BOOT_FLOW\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"END\r\n"
)

class Code:
    def __init__(self):
        self.data = bytearray()
        self.labels: dict[str, int] = {}
        self.fixups: list[tuple[int, int, str, int]] = []

    def pos(self) -> int:
        return len(self.data)

    def emit(self, data: bytes) -> None:
        self.data += bytes(data)

    def label(self, name: str) -> None:
        self.labels[name] = self.pos()

    def rel32(self, opcode: bytes, label: str) -> None:
        self.emit(opcode)
        off = self.pos()
        self.emit(b"\x00" * 4)
        self.fixups.append((off, self.pos(), label, 4))

    def rel8(self, opcode: int, label: str) -> None:
        self.emit(bytes((opcode, 0)))
        self.fixups.append((self.pos() - 1, self.pos(), label, 1))

    def lea_rsi(self, label: str) -> None:
        self.emit(b"\x48\x8d\x35")
        off = self.pos()
        self.emit(b"\x00" * 4)
        self.fixups.append((off, self.pos(), label, 4))

    def lea_rdx_rva(self, target_rva: int) -> None:
        self.emit(b"\x48\x8d\x15")
        after_rva = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", target_rva - after_rva))

    def patch(self) -> None:
        for off, after, label, width in self.fixups:
            disp = self.labels[label] - after
            if width == 4:
                struct.pack_into("<i", self.data, off, disp)
            else:
                if not -128 <= disp <= 127:
                    raise SystemExit(f"short branch overflow for {label}: {disp}")
                self.data[off] = disp & 0xFF

def build_code() -> tuple[bytes, dict[str, int]]:
    c = Code()

    # EFI x64 ABI entry:
    #   RCX = ImageHandle
    #   RDX = EFI_SYSTEM_TABLE*
    # EFI_SYSTEM_TABLE.ConIn is at +0x30 on x64.
    c.emit(b"\x49\x89\xcc")          # mov r12,rcx
    c.emit(b"\x49\x89\xd5")          # mov r13,rdx
    c.emit(b"\x4c\x8b\x72\x30")      # mov r14,[rdx+0x30]
    c.emit(b"\x48\x83\xec\x28")      # 32-byte shadow + alignment

    # COM1: 38400 8N1.
    for port, value in (
        (0x3F9, 0x00),
        (0x3FB, 0x80),
        (0x3F8, 0x03),
        (0x3F9, 0x00),
        (0x3FB, 0x03),
        (0x3FA, 0xC7),
        (0x3FC, 0x0B),
    ):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")

    def emit_frame(label: str, frame: bytes) -> None:
        c.lea_rsi(label)
        c.emit(b"\xb9" + struct.pack("<I", len(frame)))
        c.rel32(b"\xe8", "emit")

    c.label("menu")
    emit_frame("menu_frame", MENU)

    c.label("read")
    c.emit(b"\x4c\x89\xf1")          # mov rcx,r14 (ConIn this)
    c.lea_rdx_rva(DATA_RVA)          # rdx=&EFI_INPUT_KEY in RW .data
    c.emit(b"\x49\x8b\x46\x08")      # mov rax,[r14+8] ReadKeyStroke
    c.emit(b"\xff\xd0")              # call rax
    c.emit(b"\x48\x85\xc0")          # test rax,rax
    c.rel32(b"\x0f\x85", "read")     # retry on EFI_NOT_READY/nonzero

    # EFI_INPUT_KEY = UINT16 ScanCode + CHAR16 UnicodeChar.
    # MOVZX AX,[RIP+disp32] is 7 bytes: displacement is relative to the next instruction.
    c.emit(b"\x0f\xb7\x05" + struct.pack("<i", DATA_RVA + 2 - (TEXT_RVA + c.pos() + 7)))
    c.emit(b"\x66\x3d\x31\x00")      # cmp ax,'1'
    c.rel32(b"\x0f\x84", "continue")
    c.emit(b"\x66\x3d\x32\x00")      # cmp ax,'2'
    c.rel32(b"\x0f\x84", "help")
    c.emit(b"\x66\x3d\x33\x00")      # cmp ax,'3'
    c.rel32(b"\x0f\x84", "recovery")

    c.label("invalid")
    emit_frame("invalid_frame", INVALID)
    c.rel32(b"\xe9", "menu")

    c.label("help")
    emit_frame("help_frame", HELP)
    c.rel32(b"\xe9", "menu")

    c.label("recovery")
    emit_frame("recovery_frame", RECOVERY)
    c.rel32(b"\xe9", "menu")

    c.label("continue")
    emit_frame("continue_frame", CONTINUE)
    c.emit(b"\x31\xc0")              # EFI_SUCCESS
    c.emit(b"\x48\x83\xc4\x28")
    c.emit(b"\xc3")

    # emit(RSI=bytes, ECX=count), direct COM1.
    c.label("emit")
    c.emit(b"\x66\xba\xfd\x03")      # dx=LSR 0x3FD
    c.label("emit_wait")
    c.emit(b"\xec\xa8\x20")          # in al,dx ; test THR empty
    c.rel8(0x74, "emit_wait")
    c.emit(b"\x66\xba\xf8\x03")      # dx=THR 0x3F8
    c.emit(b"\x8a\x06\xee")          # mov al,[rsi] ; out dx,al
    c.emit(b"\x48\xff\xc6")          # inc rsi
    c.emit(b"\x66\xba\xfd\x03")      # restore dx=LSR before next wait
    c.emit(b"\xff\xc9")              # dec ecx
    c.rel8(0x75, "emit_wait")
    c.emit(b"\xc3")

    c.label("menu_frame")
    c.emit(MENU)
    c.label("help_frame")
    c.emit(HELP)
    c.label("recovery_frame")
    c.emit(RECOVERY)
    c.label("invalid_frame")
    c.emit(INVALID)
    c.label("continue_frame")
    c.emit(CONTINUE)

    c.patch()
    return bytes(c.data), c.labels

def put(buf: bytearray, off: int, fmt: str, *values: int) -> None:
    struct.pack_into(fmt, buf, off, *values)

def build_image() -> tuple[bytes, dict[str, int]]:
    code, labels = build_code()
    if len(code) >= 0x1000:
        raise SystemExit(f"interactive code exceeds one 4K text section: {len(code)}")

    text_raw = 0x200
    text_raw_size = ((len(code) + 0x1FF) // 0x200) * 0x200
    data_raw = text_raw + text_raw_size
    data_raw_size = 0x200
    reloc_raw = data_raw + data_raw_size
    reloc_raw_size = 0x200
    file_size = reloc_raw + reloc_raw_size
    image_size = 0x4000

    image = bytearray(file_size)
    put(image, 0x00, "<H", 0x5A4D)
    put(image, 0x3C, "<I", 0x80)

    pe = 0x80
    image[pe:pe+4] = b"PE\0\0"
    coff = pe + 4
    put(image, coff, "<HHIIIHH",
        0x8664,     # AMD64
        3,          # .text + .data + .reloc
        0, 0, 0,
        0xF0,
        0x0022)

    opt = coff + 20
    put(image, opt + 0x00, "<H", 0x20B)
    put(image, opt + 0x04, "<I", text_raw_size)
    put(image, opt + 0x08, "<I", data_raw_size + reloc_raw_size)
    put(image, opt + 0x10, "<I", TEXT_RVA)
    put(image, opt + 0x14, "<I", TEXT_RVA)
    put(image, opt + 0x18, "<Q", 0x400000)
    put(image, opt + 0x20, "<I", 0x1000)
    put(image, opt + 0x24, "<I", 0x200)
    put(image, opt + 0x38, "<I", image_size)
    put(image, opt + 0x3C, "<I", 0x200)
    put(image, opt + 0x44, "<H", 10)       # EFI application
    put(image, opt + 0x48, "<Q", 0x100000)
    put(image, opt + 0x50, "<Q", 0x1000)
    put(image, opt + 0x58, "<Q", 0x100000)
    put(image, opt + 0x60, "<Q", 0x1000)
    put(image, opt + 0x6C, "<I", 16)
    put(image, opt + 0x70 + 5 * 8, "<II", RELOC_RVA, 8)

    sec = opt + 0xF0

    image[sec:sec+8] = b".text\0\0\0"
    put(image, sec + 0x08, "<I", len(code))
    put(image, sec + 0x0C, "<I", TEXT_RVA)
    put(image, sec + 0x10, "<I", text_raw_size)
    put(image, sec + 0x14, "<I", text_raw)
    put(image, sec + 0x24, "<I", 0x60000020)  # RX, not writable

    data = sec + 40
    image[data:data+8] = b".data\0\0\0"
    put(image, data + 0x08, "<I", 4)
    put(image, data + 0x0C, "<I", DATA_RVA)
    put(image, data + 0x10, "<I", data_raw_size)
    put(image, data + 0x14, "<I", data_raw)
    put(image, data + 0x24, "<I", 0xC0000040) # RW, not executable

    reloc = sec + 80
    image[reloc:reloc+8] = b".reloc\0\0"
    put(image, reloc + 0x08, "<I", 8)
    put(image, reloc + 0x0C, "<I", RELOC_RVA)
    put(image, reloc + 0x10, "<I", reloc_raw_size)
    put(image, reloc + 0x14, "<I", reloc_raw)
    put(image, reloc + 0x24, "<I", 0x42000040)

    image[text_raw:text_raw+len(code)] = code
    image[data_raw:data_raw+4] = b"\x00\x00\x00\x00"
    put(image, reloc_raw, "<II", TEXT_RVA, 8)
    return bytes(image), labels

def validate(image: bytes) -> None:
    if image[:2] != b"MZ":
        raise SystemExit("missing MZ")
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[pe:pe+4] != b"PE\0\0":
        raise SystemExit("missing PE")
    coff = pe + 4
    machine, sections = struct.unpack_from("<HH", image, coff)
    if (machine, sections) != (0x8664, 3):
        raise SystemExit("unexpected PE machine/section count")
    opt = coff + 20
    if struct.unpack_from("<H", image, opt)[0] != 0x20B:
        raise SystemExit("not PE32+")
    if struct.unpack_from("<H", image, opt + 0x44)[0] != 10:
        raise SystemExit("not EFI application")

    sec = opt + 0xF0
    data = sec + 40
    if image[sec:sec+8].rstrip(b"\0") != b".text":
        raise SystemExit("missing text section")
    if image[data:data+8].rstrip(b"\0") != b".data":
        raise SystemExit("missing data section")
    text_chars = struct.unpack_from("<I", image, sec + 0x24)[0]
    data_chars = struct.unpack_from("<I", image, data + 0x24)[0]
    if text_chars & 0x80000000:
        raise SystemExit("text section is writable")
    if not (data_chars & 0x80000000) or (data_chars & 0x20000000):
        raise SystemExit("data section permissions invalid")

def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_uefi_interactive_access.py OUTPUT")
    image, labels = build_image()
    validate(image)
    out = Path(sys.argv[1])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print("OS_UEFI_INTERACTIVE_ACCESS_BUILD=PASS")
    print(f"bytes={len(image)}")
    print("keybuf-rva=0x2000")
    print(f"text-bytes={max(labels.values()) if labels else 0}")
    print("sha256=" + hashlib.sha256(image).hexdigest())

if __name__ == "__main__":
    main()
