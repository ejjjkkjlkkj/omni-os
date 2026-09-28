#!/usr/bin/env python3
from __future__ import annotations
import hashlib
import struct
import sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x2000
RATE=8000
TARGET_TEXT="Secure Boot desactive.\r\n"
TARGET_UTF16=TARGET_TEXT.encode("utf-16le")+b"\x00\x00"

MARK_START=b"QEVARYNOX-UEFI-CONOUT-SR-V1\r\nSTATE=HOOK_INSTALL\r\nEND\r\n"
MARK_CAPTURE=b"QEVARYNOX-UEFI-CONOUT-SR-V1\r\nEVENT=CONOUT_CAPTURE\r\nTEXT=Secure Boot desactive.\r\nEND\r\n"
MARK_SPOKEN=b"QEVARYNOX-UEFI-CONOUT-SR-V1\r\nEVENT=SPOKEN\r\nTEXT=Secure Boot desactive.\r\nSTATUS=READY\r\nEND\r\n"
MARK_DONE=b"QEVARYNOX-UEFI-CONOUT-SR-V1\r\nSTATE=HOOK_RESTORED\r\nSTATUS=PASS\r\nEND\r\n"
MARK_FAIL=b"QEVARYNOX-UEFI-CONOUT-SR-V1\r\nSTATUS=BLOCKED\r\nREASON=CONOUT_BRIDGE_FAILED\r\nEND\r\n"

def put(buf,off,fmt,*values):
    struct.pack_into(fmt,buf,off,*values)

class Code:
    def __init__(self):
        self.data=bytearray()
        self.labels={}
        self.fix=[]

    def pos(self): return len(self.data)
    def emit(self,b): self.data+=bytes(b)
    def label(self,n): self.labels[n]=self.pos()

    def rel32(self,op,label):
        self.emit(op)
        off=self.pos()
        self.emit(b"\0"*4)
        self.fix.append((off,self.pos(),label,4))

    def rel8(self,op,label):
        self.emit(bytes((op,0)))
        self.fix.append((self.pos()-1,self.pos(),label,1))

    def lea_rax_label(self,label):
        self.emit(b"\x48\x8d\x05")
        off=self.pos()
        self.emit(b"\0"*4)
        self.fix.append((off,self.pos(),label,4))

    def data_disp(self,opcode,off):
        self.emit(opcode)
        after=TEXT_RVA+self.pos()+4
        self.emit(struct.pack("<i",DATA_RVA+off-after))

    def lea_rdx_data(self,off): self.data_disp(b"\x48\x8d\x15",off)
    def lea_r9_data(self,off): self.data_disp(b"\x4c\x8d\x0d",off)
    def lea_r10_data(self,off): self.data_disp(b"\x4c\x8d\x15",off)
    def mov_rax_data(self,off): self.data_disp(b"\x48\x8b\x05",off)
    def mov_r10_data(self,off): self.data_disp(b"\x4c\x8b\x15",off)
    def mov_r11_data(self,off): self.data_disp(b"\x4c\x8b\x1d",off)
    def mov_data_rax(self,off): self.data_disp(b"\x48\x89\x05",off)

    def patch(self):
        for off,after,label,width in self.fix:
            disp=self.labels[label]-after
            if width==4:
                struct.pack_into("<i",self.data,off,disp)
            else:
                if not -128<=disp<=127:
                    raise SystemExit(f"short branch overflow {label}: {disp}")
                self.data[off]=disp&0xff

def make_pe(code:bytes, data_payload:bytes=b"", subsystem=10):
    text_raw=0x200
    text_raw_size=(len(code)+0x1ff)&~0x1ff
    has_data=bool(data_payload)
    data_raw=text_raw+text_raw_size
    data_raw_size=((len(data_payload)+0x1ff)&~0x1ff) if has_data else 0
    if has_data:
        reloc_rva=(DATA_RVA+len(data_payload)+0xfff)&~0xfff
        reloc_raw=data_raw+data_raw_size
        sections=3
    else:
        reloc_rva=(TEXT_RVA+len(code)+0xfff)&~0xfff
        reloc_raw=text_raw+text_raw_size
        sections=2
    image_size=reloc_rva+0x1000
    image=bytearray(reloc_raw+0x200)

    put(image,0x00,"<H",0x5A4D)
    put(image,0x3c,"<I",0x80)
    pe=0x80
    image[pe:pe+4]=b"PE\0\0"
    coff=pe+4
    put(image,coff,"<HHIIIHH",0x8664,sections,0,0,0,0xF0,0x22)
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
    put(image,opt+0x44,"<H",subsystem)
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

    if has_data:
        data=sec+40
        image[data:data+8]=b".data\0\0\0"
        put(image,data+0x08,"<I",len(data_payload))
        put(image,data+0x0c,"<I",DATA_RVA)
        put(image,data+0x10,"<I",data_raw_size)
        put(image,data+0x14,"<I",data_raw)
        put(image,data+0x24,"<I",0xC0000040)
        reloc=sec+80
    else:
        reloc=sec+40

    image[reloc:reloc+8]=b".reloc\0\0"
    put(image,reloc+0x08,"<I",8)
    put(image,reloc+0x0c,"<I",reloc_rva)
    put(image,reloc+0x10,"<I",0x200)
    put(image,reloc+0x14,"<I",reloc_raw)
    put(image,reloc+0x24,"<I",0x42000040)

    image[text_raw:text_raw+len(code)]=code
    if has_data:
        image[data_raw:data_raw+len(data_payload)]=data_payload
    put(image,reloc_raw,"<II",TEXT_RVA,8)
    return bytes(image)

def build_child():
    c=Code()
    # RCX=image handle, RDX=system table
    c.emit(b"\x48\x8b\x4a\x40")          # rcx=ConOut
    c.emit(b"\x48\x83\xec\x28")
    c.lea_rax_label("text")
    c.emit(b"\x48\x89\xc2")              # rdx=&CHAR16 text
    c.emit(b"\x48\x8b\x41\x08")          # OutputString
    c.emit(b"\xff\xd0")
    c.emit(b"\x31\xc0\x48\x83\xc4\x28\xc3")
    c.label("text")
    c.emit(TARGET_UTF16)
    c.patch()
    return make_pe(bytes(c.data))

def duration_us(n):
    return (n*1_000_000+RATE-1)//RATE+300_000

def build_parent(child:bytes, speech:bytes):
    if not 1000<len(speech)<60000:
        raise SystemExit(f"speech length invalid {len(speech)}")

    # Writable data layout.
    data=bytearray(0x100)
    L={
        "maxaddr":0x00,
        "orig_output":0x10,
        "conout":0x18,
        "bootservices":0x20,
        "child_handle":0x28,
        "dma":0x30,
    }
    struct.pack_into("<Q",data,L["maxaddr"],0x00FFFFFF)

    L["speech"]=len(data); data+=speech
    L["child"]=len(data); data+=child
    L["target"]=len(data); data+=TARGET_UTF16
    for n,m in (("start",MARK_START),("capture",MARK_CAPTURE),("spoken",MARK_SPOKEN),("done",MARK_DONE),("fail",MARK_FAIL)):
        L["mark:"+n]=len(data); data+=m

    c=Code()
    # EFI entry.
    c.emit(b"\x41\x54\x41\x55\x41\x56\x41\x57")  # save nonvolatile r12-r15
    c.emit(b"\x49\x89\xcc")              # r12=image handle
    c.emit(b"\x49\x89\xd5")              # r13=system table
    c.emit(b"\x4c\x8b\x72\x60")          # r14=BootServices
    c.emit(b"\x4c\x8b\x7a\x40")          # r15=ConOut
    c.emit(b"\x48\x83\xec\x38")

    # persist pointers
    c.emit(b"\x4c\x89\xf0"); c.mov_data_rax(L["bootservices"])
    c.emit(b"\x4c\x89\xf8"); c.mov_data_rax(L["conout"])

    # COM1 evidence
    for port,val in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
        c.emit(b"\x66\xba"+struct.pack("<H",port)+b"\xb0"+bytes((val,))+b"\xee")

    def serial(mark):
        c.lea_rdx_data(L["mark:"+mark])
        c.emit(b"\xb9"+struct.pack("<I",len({"start":MARK_START,"capture":MARK_CAPTURE,"spoken":MARK_SPOKEN,"done":MARK_DONE,"fail":MARK_FAIL}[mark])))
        c.rel32(b"\xe8","serial_emit")

    serial("start")

    # Allocate DMA window below 16 MiB.
    c.emit(b"\xb9\x01\x00\x00\x00")
    c.emit(b"\xba\x04\x00\x00\x00")
    c.emit(b"\x41\xb8\x20\x00\x00\x00")
    c.lea_r9_data(L["maxaddr"])
    c.emit(b"\x49\x8b\x46\x28\xff\xd0\x48\x85\xc0")
    c.rel32(b"\x0f\x85","fail")
    c.mov_rax_data(L["maxaddr"])
    c.emit(b"\x48\x05\xff\xff\x00\x00\x48\x25\x00\x00\xff\xff")
    c.mov_data_rax(L["dma"])
    c.emit(b"\x48\x3d\x00\x00\x00\x01")
    c.rel32(b"\x0f\x83","fail")

    # SB16 reset/init.
    c.emit(b"\x66\xba\x26\x02\xb0\x01\xee")
    c.emit(b"\xb9\x05\x00\x00\x00\x49\x8b\x86\xf8\x00\x00\x00\xff\xd0")
    c.emit(b"\x66\xba\x26\x02\x31\xc0\xee")
    c.emit(b"\xb9\x00\x00\x02\x00")
    c.label("reset_wait")
    c.emit(b"\x66\xba\x2e\x02\xec\xa8\x80")
    c.rel8(0x75,"reset_ready")
    c.emit(b"\xff\xc9")
    c.rel32(b"\x0f\x85","reset_wait")
    c.rel32(b"\xe9","fail")
    c.label("reset_ready")
    c.emit(b"\x66\xba\x2a\x02\xec\x3c\xaa")
    c.rel32(b"\x0f\x85","fail")
    for val in (0xd1,0x41,0x1f,0x40):
        c.emit(b"\x41\xb2"+bytes((val,)))
        c.rel32(b"\xe8","dsp_write")

    # Save original ConOut.OutputString and install hook.
    c.emit(b"\x49\x8b\x47\x08")
    c.mov_data_rax(L["orig_output"])
    c.lea_rax_label("hook")
    c.emit(b"\x49\x89\x47\x08")

    # LoadImage(FALSE,parent,NULL,embedded child,size,&child_handle)
    c.emit(b"\x31\xc9")                    # rcx=FALSE
    c.emit(b"\x4c\x89\xe2")                # rdx=parent
    c.emit(b"\x45\x31\xc0")                # r8=NULL device path
    c.lea_r9_data(L["child"])              # r9=source buffer
    c.emit(b"\x48\xc7\x44\x24\x20"+struct.pack("<I",len(child)))
    c.lea_rax_label("child_handle_ptr")
    # Replace lea label trick with data address directly.
    # Undo previous instruction by using no-op label target below is not possible; c emits separate below.
    # We deliberately use RAX as pointer then stack arg.
    c.emit(b"\x48\x89\x44\x24\x28")
    c.emit(b"\x49\x8b\x86\xc8\x00\x00\x00\xff\xd0\x48\x85\xc0")
    c.rel32(b"\x0f\x85","restore_fail")

    # StartImage(child_handle,NULL,NULL)
    c.mov_rax_data(L["child_handle"])
    c.emit(b"\x48\x89\xc1\x31\xd2\x45\x31\xc0")
    c.emit(b"\x49\x8b\x86\xd0\x00\x00\x00\xff\xd0")
    c.emit(b"\x48\x85\xc0")
    c.rel32(b"\x0f\x85","restore_fail")

    c.label("restore_ok")
    c.mov_rax_data(L["orig_output"])
    c.emit(b"\x49\x89\x47\x08")
    serial("done")
    c.emit(b"\x31\xc0\x48\x83\xc4\x38")
    c.emit(b"\x41\x5f\x41\x5e\x41\x5d\x41\x5c\xc3")

    c.label("restore_fail")
    c.mov_rax_data(L["orig_output"])
    c.emit(b"\x49\x89\x47\x08")
    c.label("fail")
    serial("fail")
    c.emit(b"\xb8\x01\x00\x00\x00\x48\x83\xc4\x38")
    c.emit(b"\x41\x5f\x41\x5e\x41\x5d\x41\x5c\xc3")

    # Hook: call original OutputString, then match target and speak it.
    c.label("hook")
    c.emit(b"\x48\x83\xec\x38")
    c.emit(b"\x48\x89\x4c\x24\x20")
    c.emit(b"\x48\x89\x54\x24\x28")
    c.mov_rax_data(L["orig_output"])
    c.emit(b"\xff\xd0")
    c.emit(b"\x48\x89\x44\x24\x30")

    c.emit(b"\x4c\x8b\x4c\x24\x28")       # r9=actual CHAR16*
    c.lea_r10_data(L["target"])            # r10=expected
    c.emit(b"\xb9"+struct.pack("<I",len(TARGET_UTF16)//2-1))
    c.label("cmp_loop")
    c.emit(b"\x41\x0f\xb7\x01")            # eax=[r9]
    c.emit(b"\x66\x41\x3b\x02")            # cmp ax,[r10]
    c.rel32(b"\x0f\x85","hook_return")
    c.emit(b"\x49\x83\xc1\x02\x49\x83\xc2\x02\xff\xc9")
    c.rel32(b"\x0f\x85","cmp_loop")

    serial("capture")
    c.lea_rdx_data(L["speech"])
    c.emit(b"\xb9"+struct.pack("<I",len(speech)))
    c.emit(b"\x41\xb8"+struct.pack("<I",duration_us(len(speech))))
    c.rel32(b"\xe8","play_clip")
    serial("spoken")

    c.label("hook_return")
    c.emit(b"\x48\x8b\x44\x24\x30\x48\x83\xc4\x38\xc3")

    # play_clip(RDX source, ECX len, R8D duration)
    c.label("play_clip")
    c.emit(b"\x53\x56\x57\x41\x54\x48\x83\xec\x28")
    c.emit(b"\x44\x89\x44\x24\x20")      # preserve R8D duration across DSP helpers
    c.emit(b"\x48\x89\xd6\x41\x89\xcc\x41\x89\xcb")
    c.mov_rax_data(L["dma"])
    c.emit(b"\x48\x89\xc3\x48\x89\xc7\xf3\xa4")

    for port,val in ((0x0a,0x05),(0x0c,0),(0x0b,0x49)):
        c.emit(b"\x66\xba"+struct.pack("<H",port)+b"\xb0"+bytes((val,))+b"\xee")
    c.emit(b"\x48\x89\xd8\x48\xc1\xe8\x10\x66\xba\x83\x00\xee")
    c.emit(b"\x48\x89\xd8\x66\xba\x02\x00\xee\x48\xc1\xe8\x08\xee")
    c.emit(b"\x44\x89\xd8\xff\xc8\x66\xba\x03\x00\xee\xc1\xe8\x08\xee")
    c.emit(b"\x66\xba\x0a\x00\xb0\x01\xee")
    for val in (0xc0,0):
        c.emit(b"\x41\xb2"+bytes((val,))); c.rel32(b"\xe8","dsp_write")
    c.emit(b"\x44\x89\xd8\xff\xc8\x41\x88\xc2"); c.rel32(b"\xe8","dsp_write")
    c.emit(b"\x44\x89\xd8\xff\xc8\xc1\xe8\x08\x41\x88\xc2"); c.rel32(b"\xe8","dsp_write")

    c.emit(b"\x8b\x4c\x24\x20")          # Stall(duration_us), not PCM byte count
    c.mov_rax_data(L["bootservices"])
    c.emit(b"\x48\x8b\x80\xf8\x00\x00\x00\xff\xd0")
    c.emit(b"\x48\x83\xc4\x28\x41\x5c\x5f\x5e\x5b\xc3")

    c.label("dsp_write")
    c.emit(b"\x66\xba\x2c\x02")
    c.label("dsp_wait")
    c.emit(b"\xec\xa8\x80")
    c.rel8(0x75,"dsp_wait")
    c.emit(b"\x44\x88\xd0\xee\xc3")

    c.label("serial_emit")
    c.emit(b"\x49\x89\xd0\x66\xba\xfd\x03")
    c.label("serial_wait")
    c.emit(b"\xec\xa8\x20")
    c.rel8(0x74,"serial_wait")
    c.emit(b"\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9")
    c.rel8(0x75,"serial_wait")
    c.emit(b"\xc3")

    # A data-address pseudo-label implemented as a tiny RIP-relative load target.
    c.label("child_handle_ptr")
    # This label must never execute; its address should be DATA_RVA+child_handle, but
    # the LoadImage argument above currently points here. Patch its address below.
    c.emit(b"\xcc")
    c.patch()

    code=bytearray(c.data)

    # Patch the one LEA RAX child_handle_ptr use to instead address writable data.
    # Find the LEA immediately after storing SourceSize: last occurrence before LoadImage.
    needle=b"\x48\x8d\x05"
    search_end=code.find(b"\x49\x8b\x86\xc8\x00\x00\x00")
    pos=code.rfind(needle,0,search_end)
    if pos<0:
        raise SystemExit("child handle LEA not found")
    after_rva=TEXT_RVA+pos+7
    struct.pack_into("<i",code,pos+3,DATA_RVA+L["child_handle"]-after_rva)

    image=make_pe(bytes(code),bytes(data))
    return image,L

def validate(parent,child,speech):
    for image,name in ((parent,"parent"),(child,"child")):
        if image[:2]!=b"MZ": raise SystemExit(name+" MZ")
        pe=struct.unpack_from("<I",image,0x3c)[0]
        if image[pe:pe+4]!=b"PE\0\0": raise SystemExit(name+" PE")
        coff=pe+4
        machine,nsects=struct.unpack_from("<HH",image,coff)
        if machine!=0x8664: raise SystemExit(name+" machine")
        opt=coff+20
        if struct.unpack_from("<H",image,opt)[0]!=0x20b: raise SystemExit(name+" PE32+")
        if struct.unpack_from("<H",image,opt+0x44)[0]!=10: raise SystemExit(name+" subsystem")

    if child not in parent:
        raise SystemExit("child image not embedded")
    if speech not in parent:
        raise SystemExit("speech not embedded")
    if TARGET_UTF16 not in child or TARGET_UTF16 not in parent:
        raise SystemExit("target text binding missing")

    pe=struct.unpack_from("<I",parent,0x3c)[0]
    sec=pe+4+20+0xF0
    data=sec+40
    text_chars=struct.unpack_from("<I",parent,sec+0x24)[0]
    data_chars=struct.unpack_from("<I",parent,data+0x24)[0]
    if text_chars&0x80000000: raise SystemExit("parent text writable")
    if not(data_chars&0x80000000) or data_chars&0x20000000:
        raise SystemExit("parent data permissions")

def main():
    if len(sys.argv)!=3:
        raise SystemExit("usage: build_conout_screenreader.py SPEECH_U8 OUTPUT_DIR")
    speech=Path(sys.argv[1]).read_bytes()
    child=build_child()
    parent,_=build_parent(child,speech)
    validate(parent,child,speech)

    out=Path(sys.argv[2]); out.mkdir(parents=True,exist_ok=True)
    (out/"BOOTX64.EFI").write_bytes(parent)
    (out/"target.efi").write_bytes(child)

    print("OS_UEFI_CONOUT_SCREENREADER_BUILD=PASS")
    print("parent-bytes="+str(len(parent)))
    print("parent-sha256="+hashlib.sha256(parent).hexdigest())
    print("child-bytes="+str(len(child)))
    print("child-sha256="+hashlib.sha256(child).hexdigest())
    print("speech-bytes="+str(len(speech)))
    print("speech-sha256="+hashlib.sha256(speech).hexdigest())
    print("target-text="+TARGET_TEXT.strip())

if __name__=="__main__":
    main()
