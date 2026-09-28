#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import struct
import sys
from pathlib import Path

TEXT_RVA = 0x1000
DATA_RVA = 0x4000
SAMPLE_RATE = 8000
MAX_CLIP = 60000

CLIP_NAMES = (
    "startup",
    "focus_accessibility",
    "focus_recovery",
    "help",
    "recovery",
    "invalid",
    "continue",
)

MARKERS = {
    "boot": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=BOOT\r\nFOCUS=CONTINUE\r\nSPEECH=DONE\r\nEND\r\n",
    "focus_accessibility": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=FOCUS\r\nFOCUS=ACCESSIBILITY\r\nSPEECH=DONE\r\nEND\r\n",
    "focus_recovery": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=FOCUS\r\nFOCUS=RECOVERY\r\nSPEECH=DONE\r\nEND\r\n",
    "focus_continue": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=FOCUS\r\nFOCUS=CONTINUE\r\nSPEECH=DONE\r\nEND\r\n",
    "help": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=ACTIVATE\r\nID=ACCESSIBILITY\r\nSTATUS=READY\r\nSPEECH=DONE\r\nEND\r\n",
    "recovery": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=ACTIVATE\r\nID=RECOVERY\r\nSTATUS=BLOCKED\r\nREASON=RECOVERY_NOT_ESTABLISHED\r\nSPEECH=DONE\r\nEND\r\n",
    "invalid": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=INVALID_KEY\r\nSTATUS=BLOCKED\r\nREASON=KEY_NOT_MAPPED\r\nSPEECH=DONE\r\nEND\r\n",
    "continue": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nEVENT=ACTIVATE\r\nID=CONTINUE\r\nSTATUS=CONFIRMED\r\nACTION=RETURN_TO_FIRMWARE_BOOT_FLOW\r\nSPEECH=DONE\r\nEND\r\n",
    "fail": b"QEVARYNOX-UEFI-SCREENREADER-INTERACTIVE-V1\r\nSTATUS=BLOCKED\r\nREASON=AUDIO_PATH_FAILED\r\nEND\r\n",
}

class Code:
    def __init__(self) -> None:
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
        self.emit(b"\0" * 4)
        self.fixups.append((off, self.pos(), label, 4))

    def rel8(self, opcode: int, label: str) -> None:
        self.emit(bytes((opcode, 0)))
        self.fixups.append((self.pos() - 1, self.pos(), label, 1))

    def lea_rsi_data(self, off: int) -> None:
        self.emit(b"\x48\x8d\x35")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def lea_r9_data(self, off: int) -> None:
        self.emit(b"\x4c\x8d\x0d")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def lea_rdx_data(self, off: int) -> None:
        self.emit(b"\x48\x8d\x15")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def mov_rax_data(self, off: int) -> None:
        self.emit(b"\x48\x8b\x05")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def mov_eax_data(self, off: int) -> None:
        self.emit(b"\x8b\x05")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def mov_data_eax(self, off: int) -> None:
        self.emit(b"\x89\x05")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def movzx_eax_data16(self, off: int) -> None:
        self.emit(b"\x0f\xb7\x05")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def patch(self) -> None:
        for off, after, label, width in self.fixups:
            disp = self.labels[label] - after
            if width == 4:
                struct.pack_into("<i", self.data, off, disp)
            else:
                if not -128 <= disp <= 127:
                    raise SystemExit(f"short branch overflow {label}: {disp}")
                self.data[off] = disp & 0xFF


def layout_data(clips: dict[str, bytes]) -> tuple[bytes, dict[str, int]]:
    data = bytearray(0x100)
    offsets: dict[str, int] = {"maxaddr": 0, "keybuf": 8, "focus": 12}
    struct.pack_into("<Q", data, offsets["maxaddr"], 0x00FFFFFF)
    struct.pack_into("<I", data, offsets["focus"], 0)

    for name in CLIP_NAMES:
        offsets["clip_" + name] = len(data)
        data += clips[name]

    for name, marker in MARKERS.items():
        offsets["marker_" + name] = len(data)
        data += marker

    return bytes(data), offsets


def build_code(clips: dict[str, bytes], off: dict[str, int]) -> bytes:
    c = Code()

    c.emit(b"\x53\x56\x57")
    c.emit(b"\x41\x54\x41\x55\x41\x56\x41\x57")
    c.emit(b"\x48\x83\xec\x20")

    c.emit(b"\x49\x89\xd5")
    c.emit(b"\x4c\x8b\x72\x30")
    c.emit(b"\x4c\x8b\x7a\x60")

    for port, value in (
        (0x3F9, 0x00), (0x3FB, 0x80), (0x3F8, 0x03),
        (0x3F9, 0x00), (0x3FB, 0x03), (0x3FA, 0xC7), (0x3FC, 0x0B),
    ):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")

    c.emit(b"\xb9\x01\x00\x00\x00")
    c.emit(b"\xba\x04\x00\x00\x00")
    c.emit(b"\x41\xb8\x20\x00\x00\x00")
    c.lea_r9_data(off["maxaddr"])
    c.emit(b"\x49\x8b\x47\x28\xff\xd0")
    c.emit(b"\x48\x85\xc0")
    c.rel32(b"\x0f\x85", "fail")

    c.mov_rax_data(off["maxaddr"])
    c.emit(b"\x48\x05\xff\xff\x00\x00")
    c.emit(b"\x48\x25\x00\x00\xff\xff")
    c.emit(b"\x48\x89\xc3")
    c.emit(b"\x48\x81\xfb\x00\x00\x00\x01")
    c.rel32(b"\x0f\x83", "fail")

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

    def serial(name: str) -> None:
        marker = MARKERS[name]
        c.lea_rsi_data(off["marker_" + name])
        c.emit(b"\xb9" + struct.pack("<I", len(marker)))
        c.rel32(b"\xe8", "serial_emit")

    def speak(name: str) -> None:
        c.lea_rsi_data(off["clip_" + name])
        c.emit(b"\xb9" + struct.pack("<I", len(clips[name])))
        c.rel32(b"\xe8", "play_pcm")

    speak("startup")
    serial("boot")
    c.rel32(b"\xe9", "read")

    c.label("read")
    c.emit(b"\x4c\x89\xf1")
    c.lea_rdx_data(off["keybuf"])
    c.emit(b"\x49\x8b\x46\x08\xff\xd0")
    c.emit(b"\x48\x85\xc0")
    c.rel32(b"\x0f\x85", "read")

    c.movzx_eax_data16(off["keybuf"] + 2)
    c.emit(b"\x66\x3d\x0d\x00")
    c.rel32(b"\x0f\x84", "activate")

    c.emit(b"\x66\x3d\x31\x00")
    c.rel32(b"\x0f\x84", "direct_continue")
    c.emit(b"\x66\x3d\x32\x00")
    c.rel32(b"\x0f\x84", "direct_help")
    c.emit(b"\x66\x3d\x33\x00")
    c.rel32(b"\x0f\x84", "direct_recovery")

    c.movzx_eax_data16(off["keybuf"])
    c.emit(b"\x66\x3d\x01\x00")
    c.rel32(b"\x0f\x84", "up")
    c.emit(b"\x66\x3d\x02\x00")
    c.rel32(b"\x0f\x84", "down")
    c.rel32(b"\xe9", "invalid")

    c.label("down")
    c.mov_eax_data(off["focus"])
    c.emit(b"\xff\xc0")
    c.emit(b"\x83\xf8\x03")
    c.rel32(b"\x0f\x82", "store_focus")
    c.emit(b"\x31\xc0")
    c.rel32(b"\xe9", "store_focus")

    c.label("up")
    c.mov_eax_data(off["focus"])
    c.emit(b"\x85\xc0")
    c.rel32(b"\x0f\x85", "up_decrement")
    c.emit(b"\xb8\x02\x00\x00\x00")
    c.rel32(b"\xe9", "store_focus")
    c.label("up_decrement")
    c.emit(b"\xff\xc8")

    c.label("store_focus")
    c.mov_data_eax(off["focus"])
    c.emit(b"\x83\xf8\x00")
    c.rel32(b"\x0f\x84", "focus_continue")
    c.emit(b"\x83\xf8\x01")
    c.rel32(b"\x0f\x84", "focus_accessibility")
    c.rel32(b"\xe9", "focus_recovery")

    c.label("focus_continue")
    speak("startup")
    serial("focus_continue")
    c.rel32(b"\xe9", "read")

    c.label("focus_accessibility")
    speak("focus_accessibility")
    serial("focus_accessibility")
    c.rel32(b"\xe9", "read")

    c.label("focus_recovery")
    speak("focus_recovery")
    serial("focus_recovery")
    c.rel32(b"\xe9", "read")

    c.label("activate")
    c.mov_eax_data(off["focus"])
    c.emit(b"\x83\xf8\x00")
    c.rel32(b"\x0f\x84", "direct_continue")
    c.emit(b"\x83\xf8\x01")
    c.rel32(b"\x0f\x84", "direct_help")
    c.rel32(b"\xe9", "direct_recovery")

    c.label("direct_help")
    speak("help")
    serial("help")
    c.rel32(b"\xe9", "read")

    c.label("direct_recovery")
    speak("recovery")
    serial("recovery")
    c.rel32(b"\xe9", "read")

    c.label("invalid")
    speak("invalid")
    serial("invalid")
    c.rel32(b"\xe9", "read")

    c.label("direct_continue")
    speak("continue")
    serial("continue")
    c.emit(b"\x31\xc0")
    c.rel32(b"\xe9", "epilogue")

    c.label("fail")
    serial("fail")
    c.emit(b"\xb8\x01\x00\x00\x00")

    c.label("epilogue")
    c.emit(b"\x48\x83\xc4\x20")
    c.emit(b"\x41\x5f\x41\x5e\x41\x5d\x41\x5c")
    c.emit(b"\x5f\x5e\x5b\xc3")

    c.label("play_pcm")
    c.emit(b"\x41\x89\xcc")
    c.emit(b"\x48\x89\xdf")
    c.emit(b"\xf3\xa4")

    for port, value in ((0x0A, 0x05), (0x0C, 0x00), (0x0B, 0x49)):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")
    c.emit(b"\x48\x89\xd8\x48\xc1\xe8\x10\x66\xba\x83\x00\xee")
    c.emit(b"\x48\x89\xd8\x66\xba\x02\x00\xee\x48\xc1\xe8\x08\xee")
    c.emit(b"\x44\x89\xe0\xff\xc8")
    c.emit(b"\x66\xba\x03\x00\xee\xc1\xe8\x08\xee")
    c.emit(b"\x66\xba\x0a\x00\xb0\x01\xee")

    for value in (0xC0, 0x00):
        c.emit(b"\x41\xb2" + bytes((value,)))
        c.rel32(b"\xe8", "dsp_write")
    c.emit(b"\x44\x89\xe0\xff\xc8")
    c.emit(b"\x41\x88\xc2")
    c.rel32(b"\xe8", "dsp_write")
    c.emit(b"\x44\x89\xe0\xff\xc8\xc1\xe8\x08")
    c.emit(b"\x41\x88\xc2")
    c.rel32(b"\xe8", "dsp_write")

    c.emit(b"\x41\x6b\xcc\x7d")
    c.emit(b"\x81\xc1\x90\xd0\x03\x00")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0")
    c.emit(b"\xc3")

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


def put(buf: bytearray, off: int, fmt: str, *values: int) -> None:
    struct.pack_into(fmt, buf, off, *values)


def build(clips: dict[str, bytes]) -> bytes:
    payload, offsets = layout_data(clips)
    code = build_code(clips, offsets)
    if len(code) > 0x3000:
        raise SystemExit(f"text too large: {len(code)}")
    if len(payload) > 0x30000:
        raise SystemExit(f"data too large: {len(payload)}")

    text_raw = 0x200
    text_raw_size = (len(code) + 0x1FF) & ~0x1FF
    data_raw = text_raw + text_raw_size
    data_raw_size = (len(payload) + 0x1FF) & ~0x1FF
    reloc_rva = (DATA_RVA + len(payload) + 0xFFF) & ~0xFFF
    reloc_raw = data_raw + data_raw_size
    file_size = reloc_raw + 0x200
    image_size = reloc_rva + 0x1000

    image = bytearray(file_size)
    put(image, 0x00, "<H", 0x5A4D)
    put(image, 0x3C, "<I", 0x80)
    pe = 0x80
    image[pe:pe+4] = b"PE\0\0"
    coff = pe + 4
    put(image, coff, "<HHIIIHH", 0x8664, 3, 0, 0, 0, 0xF0, 0x0022)

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

    data = sec + 40
    image[data:data+8] = b".data\0\0\0"
    put(image, data + 0x08, "<I", len(payload))
    put(image, data + 0x0C, "<I", DATA_RVA)
    put(image, data + 0x10, "<I", data_raw_size)
    put(image, data + 0x14, "<I", data_raw)
    put(image, data + 0x24, "<I", 0xC0000040)

    reloc = sec + 80
    image[reloc:reloc+8] = b".reloc\0\0"
    put(image, reloc + 0x08, "<I", 8)
    put(image, reloc + 0x0C, "<I", reloc_rva)
    put(image, reloc + 0x10, "<I", 0x200)
    put(image, reloc + 0x14, "<I", reloc_raw)
    put(image, reloc + 0x24, "<I", 0x42000040)

    image[text_raw:text_raw+len(code)] = code
    image[data_raw:data_raw+len(payload)] = payload
    put(image, reloc_raw, "<II", TEXT_RVA, 8)
    return bytes(image)


def validate(image: bytes, clips: dict[str, bytes]) -> None:
    if image[:2] != b"MZ":
        raise SystemExit("missing MZ")
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[pe:pe+4] != b"PE\0\0":
        raise SystemExit("missing PE")
    coff = pe + 4
    machine, sections = struct.unpack_from("<HH", image, coff)
    if (machine, sections) != (0x8664, 3):
        raise SystemExit("unexpected machine/section count")
    opt = coff + 20
    if struct.unpack_from("<H", image, opt)[0] != 0x20B:
        raise SystemExit("not PE32+")
    if struct.unpack_from("<H", image, opt + 0x44)[0] != 10:
        raise SystemExit("not EFI application")
    sec = opt + 0xF0
    data = sec + 40
    text_chars = struct.unpack_from("<I", image, sec + 0x24)[0]
    data_chars = struct.unpack_from("<I", image, data + 0x24)[0]
    if text_chars & 0x80000000:
        raise SystemExit("W+X text")
    if not (data_chars & 0x80000000) or (data_chars & 0x20000000):
        raise SystemExit("data permissions invalid")
    for name, pcm in clips.items():
        if pcm not in image:
            raise SystemExit(f"PCM asset not bound: {name}")


def main() -> None:
    if len(sys.argv) != 9:
        raise SystemExit(
            "usage: build.py STARTUP FOCUS_A11Y FOCUS_RECOVERY HELP RECOVERY INVALID CONTINUE OUTPUT"
        )
    clips = {name: Path(path).read_bytes() for name, path in zip(CLIP_NAMES, sys.argv[1:8])}
    for name, pcm in clips.items():
        if not 1 <= len(pcm) <= MAX_CLIP:
            raise SystemExit(f"{name} clip length {len(pcm)} invalid")
    image = build(clips)
    validate(image, clips)
    out = Path(sys.argv[8])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print("OS_UEFI_SCREENREADER_INTERACTIVE_BUILD=PASS")
    print("image-sha256=" + hashlib.sha256(image).hexdigest())
    print("bytes=" + str(len(image)))
    print("sample-rate=8000")
    for name in CLIP_NAMES:
        print(f"{name}-bytes={len(clips[name])}")


if __name__ == "__main__":
    main()
