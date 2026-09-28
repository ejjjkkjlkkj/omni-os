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

MARK_START = (
    b"QEVARYNOX-UEFI-SCREENREADER-V1\r\n"
    b"SPEECH=MENU\r\n"
    b"AUDIO=SB16_DMA_U8_8000\r\n"
    b"VISION_REQUIRED=0\r\n"
    b"POINTER_REQUIRED=0\r\n"
    b"END\r\n"
)
MARK_DONE = (
    b"QEVARYNOX-UEFI-SCREENREADER-V1\r\n"
    b"SPEECH=MENU_DONE\r\n"
    b"STATUS=READY\r\n"
    b"END\r\n"
)
MARK_FAIL = (
    b"QEVARYNOX-UEFI-SCREENREADER-V1\r\n"
    b"STATUS=BLOCKED\r\n"
    b"REASON=AUDIO_PATH_FAILED\r\n"
    b"END\r\n"
)

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

    def lea_r9_data(self, off):
        self.emit(b"\x4c\x8d\x0d")
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def mov_rax_data(self, off):
        self.emit(b"\x48\x8b\x05")
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

def build_code(clip_len, pcm_off, start_off, done_off, fail_off):
    c = Code()

    # EFI x64 entry: RCX=ImageHandle, RDX=EFI_SYSTEM_TABLE*.
    c.emit(b"\x49\x89\xcc")
    c.emit(b"\x49\x89\xd5")
    c.emit(b"\x4c\x8b\x7a\x60")
    c.emit(b"\x48\x83\xec\x28")

    # COM1 is evidence only, never the user-facing speech channel.
    for port, value in (
        (0x3F9, 0x00), (0x3FB, 0x80), (0x3F8, 0x03),
        (0x3F9, 0x00), (0x3FB, 0x03), (0x3FA, 0xC7), (0x3FC, 0x0B),
    ):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")

    c.lea_rsi_data(start_off)
    c.emit(b"\xb9" + struct.pack("<I", len(MARK_START)))
    c.rel32(b"\xe8", "serial_emit")

    # Allocate 128 KiB below 16 MiB, then select one 64 KiB-aligned DMA window.
    # AllocatePages(AllocateMaxAddress, EfiBootServicesData, 32, &maxaddr)
    c.emit(b"\xb9\x01\x00\x00\x00")
    c.emit(b"\xba\x04\x00\x00\x00")
    c.emit(b"\x41\xb8\x20\x00\x00\x00")
    c.lea_r9_data(0)
    c.emit(b"\x49\x8b\x47\x28")
    c.emit(b"\xff\xd0")
    c.emit(b"\x48\x85\xc0")
    c.rel32(b"\x0f\x85", "fail")

    c.mov_rax_data(0)
    c.emit(b"\x48\x05\xff\xff\x00\x00")
    c.emit(b"\x48\x25\x00\x00\xff\xff")
    c.emit(b"\x48\x89\xc3")
    c.emit(b"\x48\x81\xfb\x00\x00\x00\x01")
    c.rel32(b"\x0f\x83", "fail")

    # Copy the embedded speech clip into the ISA-DMA-safe low-memory buffer.
    c.lea_rsi_data(pcm_off)
    c.emit(b"\x48\x89\xdf")
    c.emit(b"\xb9" + struct.pack("<I", clip_len))
    c.emit(b"\xf3\xa4")

    # Reset the Sound Blaster DSP.
    c.emit(b"\x66\xba\x26\x02\xb0\x01\xee")
    c.emit(b"\xb9\x05\x00\x00\x00")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00")
    c.emit(b"\xff\xd0")
    c.emit(b"\x66\xba\x26\x02\x31\xc0\xee")

    # Wait for DSP reset acknowledgement 0xAA.
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

    # Speaker on and 8000 Hz output rate.
    for value in (0xD1, 0x41, 0x1F, 0x40):
        c.emit(b"\x41\xb2" + bytes((value,)))
        c.rel32(b"\xe8", "dsp_write")

    # ISA DMA channel 1: memory -> Sound Blaster, single-cycle.
    for port, value in ((0x0A, 0x05), (0x0C, 0x00), (0x0B, 0x49)):
        c.emit(b"\x66\xba" + struct.pack("<H", port) + b"\xb0" + bytes((value,)) + b"\xee")

    c.emit(b"\x48\x89\xd8\x48\xc1\xe8\x10\x66\xba\x83\x00\xee")
    c.emit(b"\x48\x89\xd8\x66\xba\x02\x00\xee\x48\xc1\xe8\x08\xee")
    c.emit(b"\xb8" + struct.pack("<I", clip_len - 1) + b"\x66\xba\x03\x00\xee\xc1\xe8\x08\xee")
    c.emit(b"\x66\xba\x0a\x00\xb0\x01\xee")

    # SB16 generic 8-bit unsigned mono single-cycle playback.
    for value in (0xC0, 0x00, (clip_len - 1) & 0xFF, ((clip_len - 1) >> 8) & 0xFF):
        c.emit(b"\x41\xb2" + bytes((value,)))
        c.rel32(b"\xe8", "dsp_write")

    duration_us = (clip_len * 1_000_000 + SAMPLE_RATE - 1) // SAMPLE_RATE + 400_000
    c.emit(b"\xb9" + struct.pack("<I", duration_us))
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00")
    c.emit(b"\xff\xd0")

    c.lea_rsi_data(done_off)
    c.emit(b"\xb9" + struct.pack("<I", len(MARK_DONE)))
    c.rel32(b"\xe8", "serial_emit")
    c.emit(b"\x31\xc0\x48\x83\xc4\x28\xc3")

    c.label("fail")
    c.lea_rsi_data(fail_off)
    c.emit(b"\xb9" + struct.pack("<I", len(MARK_FAIL)))
    c.rel32(b"\xe8", "serial_emit")
    c.emit(b"\xb8\x01\x00\x00\x00\x48\x83\xc4\x28\xc3")

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

def build(pcm):
    if not 1 <= len(pcm) <= MAX_CLIP:
        raise SystemExit(f"clip length {len(pcm)} invalid")

    control = bytearray(0x100)
    struct.pack_into("<Q", control, 0, 0x00FFFFFF)
    pcm_off = 0x100
    start_off = pcm_off + len(pcm)
    done_off = start_off + len(MARK_START)
    fail_off = done_off + len(MARK_DONE)
    data_payload = bytes(control) + pcm + MARK_START + MARK_DONE + MARK_FAIL

    data_raw_size = (len(data_payload) + 0x1FF) & ~0x1FF
    data_vsize = len(data_payload)
    reloc_rva = (DATA_RVA + data_vsize + 0xFFF) & ~0xFFF

    code = build_code(len(pcm), pcm_off, start_off, done_off, fail_off)
    if len(code) > 0x1000:
        raise SystemExit("text too large")

    text_raw_size = (len(code) + 0x1FF) & ~0x1FF
    text_raw = 0x200
    data_raw = text_raw + text_raw_size
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
    put(image, data + 0x08, "<I", data_vsize)
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
    image[data_raw:data_raw+len(data_payload)] = data_payload
    put(image, reloc_raw, "<II", TEXT_RVA, 8)

    return bytes(image)

def validate(image, pcm):
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
    if pcm not in image:
        raise SystemExit("PCM speech asset not bound into image")

def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: build_uefi_screenreader.py PCM_U8 OUTPUT_EFI")
    pcm = Path(sys.argv[1]).read_bytes()
    image = build(pcm)
    validate(image, pcm)
    out = Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print("OS_UEFI_SCREENREADER_BUILD=PASS")
    print("pcm-sha256=" + hashlib.sha256(pcm).hexdigest())
    print("image-sha256=" + hashlib.sha256(image).hexdigest())
    print("bytes=" + str(len(image)))
    print("sample-rate=8000")
    print("format=u8-mono")

if __name__ == "__main__":
    main()
