#!/usr/bin/env python3
from __future__ import annotations
import hashlib
import struct
import sys
from pathlib import Path

TEXT_RVA = 0x1000
DATA_RVA = 0x2000
SAMPLE_RATE = 8000
MAX_CLIP = 60000

MARKERS = {
    "boot": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nSTATE=START\r\nEND\r\n",
    "menu": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nEVENT=SPEAK_MENU\r\nEND\r\n",
    "await": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nSTATE=AWAIT_KEY\r\nEND\r\n",
    "help": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nEVENT=SPEAK_HELP\r\nEND\r\n",
    "recovery": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nEVENT=SPEAK_RECOVERY\r\nEND\r\n",
    "invalid": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nEVENT=SPEAK_INVALID\r\nEND\r\n",
    "continue": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nEVENT=SPEAK_CONTINUE\r\nEND\r\n",
    "done": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nSTATUS=CONFIRMED\r\nEND\r\n",
    "fail": b"QEVARYNOX-UEFI-SCREENREADER-V2\r\nSTATUS=BLOCKED\r\nREASON=AUDIO_PATH_FAILED\r\nEND\r\n",
}
CLIP_NAMES = ("menu", "help", "recovery", "continue", "invalid")

class Code:
    def __init__(self):
        self.data = bytearray()
        self.labels = {}
        self.fix = []

    def pos(self):
        return len(self.data)

    def emit(self, data):
        self.data += bytes(data)

    def label(self, name):
        self.labels[name] = self.pos()

    def rel32(self, opcode, label):
        self.emit(opcode)
        off = self.pos()
        self.emit(b"\0" * 4)
        self.fix.append((off, self.pos(), label, 4))

    def rel8(self, opcode, label):
        self.emit(bytes((opcode, 0)))
        self.fix.append((self.pos() - 1, self.pos(), label, 1))

    def lea_rsi_data(self, off):
        self.emit(b"\x48\x8d\x35")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def lea_rdx_data(self, off):
        self.emit(b"\x48\x8d\x15")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def lea_r9_data(self, off):
        self.emit(b"\x4c\x8d\x0d")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def mov_rax_data(self, off):
        self.emit(b"\x48\x8b\x05")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def movzx_eax_word_data(self, off):
        self.emit(b"\x0f\xb7\x05")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def patch(self):
        for off, after, label, width in self.fix:
            disp = self.labels[label] - after
            if width == 4:
                struct.pack_into("<i", self.data, off, disp)
            else:
                if not -128 <= disp <= 127:
                    raise SystemExit(f"short branch overflow {label}: {disp}")
                self.data[off] = disp & 0xFF

def duration_us(n):
    return (n * 1_000_000 + SAMPLE_RATE - 1) // SAMPLE_RATE + 250_000

def build_code(layout, clips):
    c = Code()

    # EFI x64 entry: RCX=ImageHandle, RDX=EFI_SYSTEM_TABLE*.
    c.emit(b"\x49\x89\xcc\x49\x89\xd5")
    c.emit(b"\x4c\x8b\x72\x30")  # ConIn
    c.emit(b"\x4c\x8b\x7a\x60")  # BootServices
    c.emit(b"\x48\x83\xec\x28")

    # COM1 technical evidence channel.
    for port, value in (
        (0x3F9, 0x00), (0x3FB, 0x80), (0x3F8, 0x03),
        (0x3F9, 0x00), (0x3FB, 0x03), (0x3FA, 0xC7), (0x3FC, 0x0B),
    ):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")

    def serial(name):
        c.lea_rsi_data(layout["marker:" + name])
        c.emit(b"\xb9" + struct.pack("<I", len(MARKERS[name])))
        c.rel32(b"\xe8", "serial_emit")

    def play(name):
        clip = clips[name]
        c.lea_rsi_data(layout["clip:" + name])
        c.emit(b"\xb9" + struct.pack("<I", len(clip)))
        c.emit(b"\x41\xb9" + struct.pack("<I", duration_us(len(clip))))
        c.rel32(b"\xe8", "play_clip")

    serial("boot")

    # Allocate 128 KiB below 16 MiB and select one 64 KiB-aligned DMA window.
    c.emit(b"\xb9\x01\x00\x00\x00")
    c.emit(b"\xba\x04\x00\x00\x00")
    c.emit(b"\x41\xb8\x20\x00\x00\x00")
    c.lea_r9_data(layout["maxaddr"])
    c.emit(b"\x49\x8b\x47\x28\xff\xd0\x48\x85\xc0")
    c.rel32(b"\x0f\x85", "fail")

    c.mov_rax_data(layout["maxaddr"])
    c.emit(b"\x48\x05\xff\xff\x00\x00")
    c.emit(b"\x48\x25\x00\x00\xff\xff")
    c.emit(b"\x48\x89\xc3")
    c.emit(b"\x48\x81\xfb\x00\x00\x00\x01")
    c.rel32(b"\x0f\x83", "fail")

    # Initialize SB16 once.
    c.emit(b"\x66\xba\x26\x02\xb0\x01\xee")
    c.emit(b"\xb9\x05\x00\x00\x00")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0")
    c.emit(b"\x66\xba\x26\x02\x31\xc0\xee")

    c.emit(b"\xb9\x00\x00\x02\x00")
    c.label("reset_wait")
    c.emit(b"\x66\xba\x2e\x02\xec\xa8\x80")
    c.rel8(0x75, "reset_ready")
    c.emit(b"\xff\xc9")
    c.rel32(b"\x0f\x85", "reset_wait")
    c.rel32(b"\xe9", "fail")

    c.label("reset_ready")
    c.emit(b"\x66\xba\x2a\x02\xec\x3c\xaa")
    c.rel32(b"\x0f\x85", "fail")

    for value in (0xD1, 0x41, 0x1F, 0x40):
        c.emit(b"\x41\xb2" + bytes((value,)))
        c.rel32(b"\xe8", "dsp_write")

    # Speak menu, then accept native UEFI keys.
    c.label("menu")
    serial("menu")
    play("menu")
    serial("await")

    c.label("read")
    c.emit(b"\x4c\x89\xf1")
    c.lea_rdx_data(layout["keybuf"])
    c.emit(b"\x49\x8b\x46\x08\xff\xd0\x48\x85\xc0")
    c.rel32(b"\x0f\x85", "read")

    c.movzx_eax_word_data(layout["keybuf"] + 2)
    c.emit(b"\x66\x3d\x31\x00")
    c.rel32(b"\x0f\x84", "continue")
    c.emit(b"\x66\x3d\x32\x00")
    c.rel32(b"\x0f\x84", "help")
    c.emit(b"\x66\x3d\x33\x00")
    c.rel32(b"\x0f\x84", "recovery")

    c.label("invalid")
    serial("invalid")
    play("invalid")
    c.rel32(b"\xe9", "menu")

    c.label("help")
    serial("help")
    play("help")
    c.rel32(b"\xe9", "menu")

    c.label("recovery")
    serial("recovery")
    play("recovery")
    c.rel32(b"\xe9", "menu")

    c.label("continue")
    serial("continue")
    play("continue")
    serial("done")
    c.emit(b"\x31\xc0\x48\x83\xc4\x28\xc3")

    c.label("fail")
    serial("fail")
    c.emit(b"\xb8\x01\x00\x00\x00\x48\x83\xc4\x28\xc3")

    # play_clip: RSI=PCM source, ECX=length, R9D=duration_us, RBX=DMA buffer.
    c.label("play_clip")
    c.emit(b"\x41\x89\xc8")
    c.emit(b"\x45\x89\xc3\x41\xff\xcb")
    c.emit(b"\x48\x89\xdf\xf3\xa4")

    for port, value in ((0x0A, 0x05), (0x0C, 0x00), (0x0B, 0x49)):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")

    c.emit(b"\x48\x89\xd8\x48\xc1\xe8\x10\x66\xba\x83\x00\xee")
    c.emit(b"\x48\x89\xd8\x66\xba\x02\x00\xee\x48\xc1\xe8\x08\xee")
    c.emit(b"\x44\x89\xd8\x66\xba\x03\x00\xee\xc1\xe8\x08\xee")
    c.emit(b"\x66\xba\x0a\x00\xb0\x01\xee")

    for value in (0xC0, 0x00):
        c.emit(b"\x41\xb2" + bytes((value,)))
        c.rel32(b"\xe8", "dsp_write")

    c.emit(b"\x45\x88\xda")
    c.rel32(b"\xe8", "dsp_write")
    c.emit(b"\x41\xc1\xeb\x08\x45\x88\xda")
    c.rel32(b"\xe8", "dsp_write")

    c.emit(b"\x44\x89\xc9")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0\xc3")

    c.label("dsp_write")
    c.emit(b"\x66\xba\x2c\x02")
    c.label("dsp_wait")
    c.emit(b"\xec\xa8\x80")
    c.rel8(0x75, "dsp_wait")
    c.emit(b"\x44\x88\xd0\xee\xc3")

    c.label("serial_emit")
    c.emit(b"\x66\xba\xfd\x03")
    c.label("serial_wait")
    c.emit(b"\xec\xa8\x20")
    c.rel8(0x74, "serial_wait")
    c.emit(b"\x66\xba\xf8\x03\x8a\x06\xee\x48\xff\xc6\x66\xba\xfd\x03\xff\xc9")
    c.rel8(0x75, "serial_wait")
    c.emit(b"\xc3")

    c.patch()
    return bytes(c.data)

def put(buf, off, fmt, *values):
    struct.pack_into(fmt, buf, off, *values)

def build(clips):
    for name, clip in clips.items():
        if not 1 <= len(clip) <= MAX_CLIP:
            raise SystemExit(f"{name} clip length invalid: {len(clip)}")

    data = bytearray(0x100)
    layout = {"maxaddr": 0, "keybuf": 0x10}
    struct.pack_into("<Q", data, 0, 0x00FFFFFF)

    for name in CLIP_NAMES:
        layout["clip:" + name] = len(data)
        data += clips[name]

    for name, marker in MARKERS.items():
        layout["marker:" + name] = len(data)
        data += marker

    code = build_code(layout, clips)
    if len(code) > 0x1000:
        raise SystemExit(f"text too large: {len(code)}")

    text_raw_size = (len(code) + 0x1FF) & ~0x1FF
    data_raw_size = (len(data) + 0x1FF) & ~0x1FF
    text_raw = 0x200
    data_raw = text_raw + text_raw_size
    reloc_rva = (DATA_RVA + len(data) + 0xFFF) & ~0xFFF
    reloc_raw = data_raw + data_raw_size
    image_size = reloc_rva + 0x1000

    image = bytearray(reloc_raw + 0x200)
    put(image, 0x00, "<H", 0x5A4D)
    put(image, 0x3C, "<I", 0x80)

    pe = 0x80
    image[pe:pe+4] = b"PE\0\0"
    coff = pe + 4
    put(image, coff, "<HHIIIHH", 0x8664, 3, 0, 0, 0, 0xF0, 0x22)

    opt = coff + 20
    put(image, opt + 0x00, "<H", 0x20B)
    put(image, opt + 0x04, "<I", text_raw_size)
    put(image, opt + 0x08, "<I", data_raw_size + 0x200)
    put(image, opt + 0x10, "<I", TEXT_RVA)
    put(image, opt + 0x14, "<I", TEXT_RVA)
    put(image, opt + 0x18, "<Q", 0x400000)
    put(image, opt + 0x20, "<I", 0x1000)
    put(image, opt + 0x24, "<I", 0x200)
    put(image, opt + 0x38, "<I", image_size)
    put(image, opt + 0x3C, "<I", 0x200)
    put(image, opt + 0x44, "<H", 10)
    put(image, opt + 0x48, "<Q", 0x100000)
    put(image, opt + 0x50, "<Q", 0x1000)
    put(image, opt + 0x58, "<Q", 0x100000)
    put(image, opt + 0x60, "<Q", 0x1000)
    put(image, opt + 0x6C, "<I", 16)
    put(image, opt + 0x70 + 5 * 8, "<II", reloc_rva, 8)

    sec = opt + 0xF0
    image[sec:sec+8] = b".text\0\0\0"
    put(image, sec + 0x08, "<I", len(code))
    put(image, sec + 0x0C, "<I", TEXT_RVA)
    put(image, sec + 0x10, "<I", text_raw_size)
    put(image, sec + 0x14, "<I", text_raw)
    put(image, sec + 0x24, "<I", 0x60000020)

    data_sec = sec + 40
    image[data_sec:data_sec+8] = b".data\0\0\0"
    put(image, data_sec + 0x08, "<I", len(data))
    put(image, data_sec + 0x0C, "<I", DATA_RVA)
    put(image, data_sec + 0x10, "<I", data_raw_size)
    put(image, data_sec + 0x14, "<I", data_raw)
    put(image, data_sec + 0x24, "<I", 0xC0000040)

    reloc = sec + 80
    image[reloc:reloc+8] = b".reloc\0\0"
    put(image, reloc + 0x08, "<I", 8)
    put(image, reloc + 0x0C, "<I", reloc_rva)
    put(image, reloc + 0x10, "<I", 0x200)
    put(image, reloc + 0x14, "<I", reloc_raw)
    put(image, reloc + 0x24, "<I", 0x42000040)

    image[text_raw:text_raw+len(code)] = code
    image[data_raw:data_raw+len(data)] = data
    put(image, reloc_raw, "<II", TEXT_RVA, 8)
    return bytes(image), layout

def validate(image, clips):
    if image[:2] != b"MZ":
        raise SystemExit("missing MZ")
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[pe:pe+4] != b"PE\0\0":
        raise SystemExit("missing PE")
    coff = pe + 4
    machine, sections = struct.unpack_from("<HH", image, coff)
    if (machine, sections) != (0x8664, 3):
        raise SystemExit("unexpected PE machine/sections")
    opt = coff + 20
    if struct.unpack_from("<H", image, opt)[0] != 0x20B:
        raise SystemExit("not PE32+")
    if struct.unpack_from("<H", image, opt + 0x44)[0] != 10:
        raise SystemExit("not EFI application")

    sec = opt + 0xF0
    data_sec = sec + 40
    text_chars = struct.unpack_from("<I", image, sec + 0x24)[0]
    data_chars = struct.unpack_from("<I", image, data_sec + 0x24)[0]
    if text_chars & 0x80000000:
        raise SystemExit("text is writable")
    if not (data_chars & 0x80000000) or (data_chars & 0x20000000):
        raise SystemExit("data permissions invalid")

    for name, clip in clips.items():
        if clip not in image:
            raise SystemExit(f"{name} speech clip not bound")

def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: build_uefi_screenreader_v2.py ASSET_DIR OUTPUT_EFI")

    root = Path(sys.argv[1])
    clips = {name: (root / f"{name}.u8").read_bytes() for name in CLIP_NAMES}
    image, layout = build(clips)
    validate(image, clips)

    out = Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)

    print("OS_UEFI_SCREENREADER_V2_BUILD=PASS")
    print("image-sha256=" + hashlib.sha256(image).hexdigest())
    print("bytes=" + str(len(image)))
    for name in CLIP_NAMES:
        clip = clips[name]
        print(
            f"{name}-bytes={len(clip)} "
            f"sha256={hashlib.sha256(clip).hexdigest()} "
            f"duration-us={duration_us(len(clip))}"
        )
    print(layout)

if __name__ == "__main__":
    main()
