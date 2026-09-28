#!/usr/bin/env python3
from __future__ import annotations
import hashlib
import struct
import sys
from pathlib import Path

TEXT_RVA = 0x1000
DATA_RVA = 0x2000

MARKS = {
    "start": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nSTATE=START\r\nTRANSPORT=PCI_CFG_MMIO\r\nEND\r\n",
    "found": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nHDA_PCI_CLASS=PASS\r\nEND\r\n",
    "bar": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nHDA_BAR0_MMIO=PASS\r\nGCAP_VERSION=PASS\r\nEND\r\n",
    "reset": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nHDA_CRST=PASS\r\nEND\r\n",
    "codec": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nHDA_CODEC_PRESENT=PASS\r\nEND\r\n",
    "done": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nSTATUS=PASS\r\nEND\r\n",
    "no_hda": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n",
    "bad_bar": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_BAR_INVALID\r\nEND\r\n",
    "reset_fail": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_RESET_FAILED\r\nEND\r\n",
    "no_codec": b"QEVARYNOX-UEFI-HDA-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CODEC_NOT_PRESENT\r\nEND\r\n",
}

class Code:
    def __init__(self):
        self.data = bytearray()
        self.labels = {}
        self.fix = []

    def pos(self): return len(self.data)
    def emit(self, data): self.data += bytes(data)
    def label(self, name): self.labels[name] = self.pos()

    def rel32(self, op, label):
        self.emit(op)
        off = self.pos()
        self.emit(b"\0"*4)
        self.fix.append((off, self.pos(), label, 4))

    def rel8(self, op, label):
        self.emit(bytes((op,0)))
        self.fix.append((self.pos()-1, self.pos(), label, 1))

    def data_disp(self, opcode, off):
        self.emit(opcode)
        after = TEXT_RVA + self.pos() + 4
        self.emit(struct.pack("<i", DATA_RVA + off - after))

    def lea_rdx_data(self, off): self.data_disp(b"\x48\x8d\x15", off)

    def patch(self):
        for off, after, label, width in self.fix:
            disp = self.labels[label] - after
            if width == 4:
                struct.pack_into("<i", self.data, off, disp)
            else:
                if not -128 <= disp <= 127:
                    raise SystemExit(f"short branch overflow {label}: {disp}")
                self.data[off] = disp & 0xff

def put(buf, off, fmt, *values):
    struct.pack_into(fmt, buf, off, *values)

def build():
    data = bytearray()
    layout = {}
    for name, raw in MARKS.items():
        layout[name] = len(data)
        data += raw

    c = Code()
    c.emit(b"\x53\x41\x54\x41\x55\x41\x56\x41\x57")
    c.emit(b"\x4c\x8b\x7a\x60")
    c.emit(b"\x48\x83\xec\x20")
    c.emit(b"\xfc")

    for port, value in (
        (0x3F9,0x00),(0x3FB,0x80),(0x3F8,0x03),
        (0x3F9,0x00),(0x3FB,0x03),(0x3FA,0xC7),(0x3FC,0x0B),
    ):
        c.emit(b"\x66\xba"+struct.pack("<H",port)+b"\xb0"+bytes((value,))+b"\xee")

    def serial(name):
        c.lea_rdx_data(layout[name])
        c.emit(b"\xb9"+struct.pack("<I",len(MARKS[name])))
        c.rel32(b"\xe8","serial_emit")

    serial("start")

    c.emit(b"\x45\x31\xe4")
    c.label("scan")
    c.emit(b"\x44\x89\xe0")
    c.emit(b"\xc1\xe0\x08")
    c.emit(b"\x0d\x00\x00\x00\x80")
    c.emit(b"\x41\x89\xc5")
    c.rel32(b"\xe8","pci_read32")
    c.emit(b"\x66\x3d\xff\xff")
    c.rel32(b"\x0f\x84","scan_next")

    c.emit(b"\x44\x89\xe8")
    c.emit(b"\x83\xc8\x08")
    c.rel32(b"\xe8","pci_read32")
    c.emit(b"\xc1\xe8\x10")
    c.emit(b"\x66\x3d\x03\x04")
    c.rel32(b"\x0f\x84","found")

    c.label("scan_next")
    c.emit(b"\x41\xff\xc4")
    c.emit(b"\x41\x81\xfc\x00\x00\x01\x00")  # 65536 bus/device/function tuples in segment 0
    c.rel32(b"\x0f\x82","scan")
    c.rel32(b"\xe9","fail_no_hda")

    c.label("found")
    serial("found")

    c.emit(b"\x44\x89\xe8")
    c.emit(b"\x83\xc8\x04")
    c.rel32(b"\xe8","pci_read32")
    c.emit(b"\x89\xc1")
    c.emit(b"\x83\xc9\x06")
    c.emit(b"\x44\x89\xe8")
    c.emit(b"\x83\xc8\x04")
    c.rel32(b"\xe8","pci_write32")

    c.emit(b"\x44\x89\xe8")
    c.emit(b"\x83\xc8\x10")
    c.rel32(b"\xe8","pci_read32")
    c.emit(b"\x41\x89\xc6")
    c.emit(b"\xa8\x01")
    c.rel32(b"\x0f\x85","fail_bad_bar")
    c.emit(b"\x89\xc1")
    c.emit(b"\x83\xe1\x06")
    c.emit(b"\x83\xf9\x04")
    c.rel32(b"\x0f\x84","bar_64")
    c.emit(b"\x85\xc9")
    c.rel32(b"\x0f\x85","fail_bad_bar")
    c.rel32(b"\xe9","bar_low")

    c.label("bar_64")
    c.emit(b"\x44\x89\xe8")
    c.emit(b"\x83\xc8\x14")
    c.rel32(b"\xe8","pci_read32")
    c.emit(b"\x48\xc1\xe0\x20")
    c.emit(b"\x49\x09\xc6")

    c.label("bar_low")
    c.emit(b"\x4c\x89\xf0")
    c.emit(b"\x48\x83\xe0\xf0")
    c.emit(b"\x48\x85\xc0")
    c.rel32(b"\x0f\x84","fail_bad_bar")
    c.emit(b"\x48\x89\xc3")

    c.emit(b"\x0f\xb7\x03")
    c.emit(b"\x85\xc0")
    c.rel32(b"\x0f\x84","fail_bad_bar")
    c.emit(b"\x0f\xb6\x43\x03")
    c.emit(b"\x85\xc0")
    c.rel32(b"\x0f\x84","fail_bad_bar")
    serial("bar")

    c.emit(b"\x8b\x43\x08")
    c.emit(b"\x83\xe0\xfe")
    c.emit(b"\x89\x43\x08")
    c.emit(b"\xb9\xa0\x86\x01\x00")
    c.label("reset_clear_poll")
    c.emit(b"\x8b\x43\x08\xa8\x01")
    c.rel32(b"\x0f\x84","reset_clear_ok")
    c.emit(b"\xff\xc9")
    c.rel32(b"\x0f\x85","reset_clear_poll")
    c.rel32(b"\xe9","fail_reset")

    c.label("reset_clear_ok")
    c.emit(b"\xb9\x64\x00\x00\x00")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0")

    c.emit(b"\x8b\x43\x08")
    c.emit(b"\x83\xc8\x01")
    c.emit(b"\x89\x43\x08")
    c.emit(b"\xb9\xa0\x86\x01\x00")
    c.label("reset_set_poll")
    c.emit(b"\x8b\x43\x08\xa8\x01")
    c.rel32(b"\x0f\x85","reset_set_ok")
    c.emit(b"\xff\xc9")
    c.rel32(b"\x0f\x85","reset_set_poll")
    c.rel32(b"\xe9","fail_reset")

    c.label("reset_set_ok")
    c.emit(b"\xb9\xe8\x03\x00\x00")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0")
    serial("reset")

    c.emit(b"\x41\xbc\x64\x00\x00\x00")
    c.label("codec_poll")
    c.emit(b"\x0f\xb7\x43\x0e")
    c.emit(b"\x66\x85\xc0")
    c.rel32(b"\x0f\x85","codec_ok")
    c.emit(b"\xb9\xe8\x03\x00\x00")
    c.emit(b"\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0")
    c.emit(b"\x41\xff\xcc")
    c.rel32(b"\x0f\x85","codec_poll")
    c.rel32(b"\xe9","fail_no_codec")

    c.label("codec_ok")
    serial("codec")
    serial("done")
    c.emit(b"\x31\xc0")
    c.rel32(b"\xe9","return")

    c.label("fail_no_hda")
    serial("no_hda")
    c.rel32(b"\xe9","return_fail")
    c.label("fail_bad_bar")
    serial("bad_bar")
    c.rel32(b"\xe9","return_fail")
    c.label("fail_reset")
    serial("reset_fail")
    c.rel32(b"\xe9","return_fail")
    c.label("fail_no_codec")
    serial("no_codec")

    c.label("return_fail")
    c.emit(b"\xb8\x01\x00\x00\x00")
    c.label("return")
    c.emit(b"\x48\x83\xc4\x20")
    c.emit(b"\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5b\xc3")

    c.label("pci_read32")
    c.emit(b"\x66\xba\xf8\x0c\xef")
    c.emit(b"\x66\xba\xfc\x0c\xed\xc3")

    c.label("pci_write32")
    c.emit(b"\x66\xba\xf8\x0c\xef")
    c.emit(b"\x89\xc8")
    c.emit(b"\x66\xba\xfc\x0c\xef\xc3")

    c.label("serial_emit")
    c.emit(b"\x49\x89\xd0\x66\xba\xfd\x03")
    c.label("serial_wait")
    c.emit(b"\xec\xa8\x20")
    c.rel8(0x74,"serial_wait")
    c.emit(b"\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9")
    c.rel8(0x75,"serial_wait")
    c.emit(b"\xc3")

    c.patch()
    code = bytes(c.data)

    text_raw = 0x200
    text_raw_size = (len(code)+0x1ff)&~0x1ff
    data_raw = text_raw + text_raw_size
    data_raw_size = (len(data)+0x1ff)&~0x1ff
    reloc_rva = (DATA_RVA+len(data)+0xfff)&~0xfff
    reloc_raw = data_raw + data_raw_size
    image = bytearray(reloc_raw+0x200)
    image_size = reloc_rva+0x1000

    put(image,0x00,"<H",0x5A4D)
    put(image,0x3c,"<I",0x80)
    pe=0x80
    image[pe:pe+4]=b"PE\0\0"
    coff=pe+4
    put(image,coff,"<HHIIIHH",0x8664,3,0,0,0,0xF0,0x22)
    opt=coff+20
    put(image,opt+0x00,"<H",0x20B)
    put(image,opt+0x04,"<I",text_raw_size)
    put(image,opt+0x08,"<I",data_raw_size+0x200)
    put(image,opt+0x10,"<I",TEXT_RVA)
    put(image,opt+0x14,"<I",TEXT_RVA)
    put(image,opt+0x18,"<Q",0x400000)
    put(image,opt+0x20,"<I",0x1000)
    put(image,opt+0x24,"<I",0x200)
    put(image,opt+0x38,"<I",image_size)
    put(image,opt+0x3c,"<I",0x200)
    put(image,opt+0x44,"<H",10)
    put(image,opt+0x48,"<Q",0x100000)
    put(image,opt+0x50,"<Q",0x1000)
    put(image,opt+0x58,"<Q",0x100000)
    put(image,opt+0x60,"<Q",0x1000)
    put(image,opt+0x6c,"<I",16)
    put(image,opt+0x70+5*8,"<II",reloc_rva,8)

    sec=opt+0xF0
    image[sec:sec+8]=b".text\0\0\0"
    put(image,sec+0x08,"<I",len(code))
    put(image,sec+0x0c,"<I",TEXT_RVA)
    put(image,sec+0x10,"<I",text_raw_size)
    put(image,sec+0x14,"<I",text_raw)
    put(image,sec+0x24,"<I",0x60000020)

    dsec=sec+40
    image[dsec:dsec+8]=b".data\0\0\0"
    put(image,dsec+0x08,"<I",len(data))
    put(image,dsec+0x0c,"<I",DATA_RVA)
    put(image,dsec+0x10,"<I",data_raw_size)
    put(image,dsec+0x14,"<I",data_raw)
    put(image,dsec+0x24,"<I",0xC0000040)

    reloc=sec+80
    image[reloc:reloc+8]=b".reloc\0\0"
    put(image,reloc+0x08,"<I",8)
    put(image,reloc+0x0c,"<I",reloc_rva)
    put(image,reloc+0x10,"<I",0x200)
    put(image,reloc+0x14,"<I",reloc_raw)
    put(image,reloc+0x24,"<I",0x42000040)

    image[text_raw:text_raw+len(code)] = code
    image[data_raw:data_raw+len(data)] = data
    put(image,reloc_raw,"<II",TEXT_RVA,8)
    return bytes(image)

def validate(image):
    if image[:2] != b"MZ":
        raise SystemExit("missing MZ")
    pe=struct.unpack_from("<I",image,0x3c)[0]
    if image[pe:pe+4] != b"PE\0\0":
        raise SystemExit("missing PE")
    coff=pe+4
    machine,sections=struct.unpack_from("<HH",image,coff)
    if (machine,sections)!=(0x8664,3):
        raise SystemExit("unexpected machine/sections")
    opt=coff+20
    if struct.unpack_from("<H",image,opt)[0] != 0x20B:
        raise SystemExit("not PE32+")
    if struct.unpack_from("<H",image,opt+0x44)[0] != 10:
        raise SystemExit("not EFI application")
    sec=opt+0xF0
    data=sec+40
    text_chars=struct.unpack_from("<I",image,sec+0x24)[0]
    data_chars=struct.unpack_from("<I",image,data+0x24)[0]
    if text_chars & 0x80000000:
        raise SystemExit("text writable")
    if not(data_chars & 0x80000000) or data_chars & 0x20000000:
        raise SystemExit("data permissions invalid")
    for raw in MARKS.values():
        if raw not in image:
            raise SystemExit("marker missing")

def main():
    if len(sys.argv)!=2:
        raise SystemExit("usage: build_uefi_hda_probe.py OUTPUT_EFI")
    image=build()
    validate(image)
    out=Path(sys.argv[1])
    out.parent.mkdir(parents=True,exist_ok=True)
    out.write_bytes(image)
    print("OS_UEFI_HDA_PROBE_BUILD=PASS")
    print("bytes="+str(len(image)))
    print("sha256="+hashlib.sha256(image).hexdigest())

if __name__=="__main__":
    main()
