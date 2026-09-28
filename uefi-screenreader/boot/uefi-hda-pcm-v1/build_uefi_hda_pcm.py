#!/usr/bin/env python3
from __future__ import annotations
import hashlib, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x2000
PCM_BYTES=49152
PCM_HALF=PCM_BYTES//2
FMT=0x0011

MARKS={
 'start': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATE=START\r\nTRANSPORT=HDA_STREAM_DMA\r\nEND\r\n',
 'hda': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nHDA_CONTROLLER_CODEC=PASS\r\nEND\r\n',
 'codec': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nQEMU_CODEC_VENDOR_1AF4=PASS\r\nEND\r\n',
 'route': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nQEMU_ROUTE_DAC2_PIN3=PASS\r\nEND\r\n',
 'bdl': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nBDL_PROGRAMMED=PASS\r\nEND\r\n',
 'run': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTREAM_DMA_STARTED=PASS\r\nEND\r\n',
 'progress': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nDMA_PROGRESS=PASS\r\nEND\r\n',
 'done': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=PASS\r\nEND\r\n',
 'no_hda': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n',
 'bad_hda': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CONTROLLER_OR_CODEC_FAILED\r\nEND\r\n',
 'verb_timeout': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=BLOCKED\r\nREASON=IMMEDIATE_COMMAND_TIMEOUT\r\nEND\r\n',
 'vendor_fail': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=BLOCKED\r\nREASON=CODEC_VENDOR_UNEXPECTED\r\nEND\r\n',
 'stream_reset_fail': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=BLOCKED\r\nREASON=STREAM_RESET_FAILED\r\nEND\r\n',
 'dma_no_progress': b'QEVARYNOX-UEFI-HDA-PCM-V1\r\nSTATUS=BLOCKED\r\nREASON=DMA_NO_PROGRESS\r\nEND\r\n',
}

class Code:
 def __init__(self): self.data=bytearray(); self.labels={}; self.fix=[]
 def pos(self): return len(self.data)
 def emit(self,b): self.data+=bytes(b)
 def label(self,n): self.labels[n]=self.pos()
 def rel32(self,op,label):
  self.emit(op); off=self.pos(); self.emit(b'\0'*4); self.fix.append((off,self.pos(),label,4))
 def rel8(self,op,label):
  self.emit(bytes((op,0))); self.fix.append((self.pos()-1,self.pos(),label,1))
 def data_disp(self,op,off):
  self.emit(op); after=TEXT_RVA+self.pos()+4; self.emit(struct.pack('<i',DATA_RVA+off-after))
 def lea_rax(self,off): self.data_disp(b'\x48\x8d\x05',off)
 def lea_rdx(self,off): self.data_disp(b'\x48\x8d\x15',off)
 def patch(self):
  for off,after,label,width in self.fix:
   d=self.labels[label]-after
   if width==4: struct.pack_into('<i',self.data,off,d)
   else:
    if not -128<=d<=127: raise SystemExit(f'short overflow {label}: {d}')
    self.data[off]=d&255

def put(b,o,f,*v): struct.pack_into(f,b,o,*v)

def pcm():
 out=bytearray()
 for frame in range(PCM_BYTES//4):
  s=6000 if frame%48<24 else -6000
  out+=struct.pack('<hh',s,s)
 return bytes(out)

def build():
 data=bytearray(); L={}
 for n,m in MARKS.items(): L[n]=len(data); data+=m
 while len(data)%128: data.append(0)
 bdl=len(data); data+=b'\0'*32
 while len(data)%128: data.append(0)
 wave=len(data); data+=pcm()

 c=Code()
 c.emit(b'\x53\x41\x54\x41\x55\x41\x56\x41\x57')
 c.emit(b'\x4c\x8b\x7a\x60\x48\x83\xec\x20\xfc')
 for p,v in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
  c.emit(b'\x66\xba'+struct.pack('<H',p)+b'\xb0'+bytes((v,))+b'\xee')
 def serial(n):
  c.lea_rdx(L[n]); c.emit(b'\xb9'+struct.pack('<I',len(MARKS[n]))); c.rel32(b'\xe8','serial_emit')
 def verb(nid,bits):
  c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d'+struct.pack('<I',(nid<<20)|bits))
  c.rel32(b'\xe8','immediate')
  c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')

 serial('start')
 c.emit(b'\x45\x31\xe4')
 c.label('scan')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x08\x0d\x00\x00\x00\x80\x41\x89\xc5')
 c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x66\x3d\xff\xff'); c.rel32(b'\x0f\x84','scan_next')
 c.emit(b'\x44\x89\xe8\x83\xc8\x08'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\xc1\xe8\x10\x66\x3d\x03\x04'); c.rel32(b'\x0f\x84','found')
 c.label('scan_next')
 c.emit(b'\x41\xff\xc4\x41\x81\xfc\x00\x00\x01\x00'); c.rel32(b'\x0f\x82','scan')
 c.rel32(b'\xe9','fail_no_hda')

 c.label('found')
 c.emit(b'\x44\x89\xe8\x83\xc8\x04'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x89\xc1\x83\xc9\x06\x44\x89\xe8\x83\xc8\x04'); c.rel32(b'\xe8','pci_write32')
 c.emit(b'\x44\x89\xe8\x83\xc8\x10'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x41\x89\xc6\xa8\x01'); c.rel32(b'\x0f\x85','fail_bad_hda')
 c.emit(b'\x89\xc1\x83\xe1\x06\x83\xf9\x04'); c.rel32(b'\x0f\x85','bar_low')
 c.emit(b'\x44\x89\xe8\x83\xc8\x14'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x48\xc1\xe0\x20\x49\x09\xc6')
 c.label('bar_low')
 c.emit(b'\x4c\x89\xf0\x48\x83\xe0\xf0\x48\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x48\x89\xc3\x0f\xb7\x03\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x0f\xb6\x43\x03\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')

 c.emit(b'\x8b\x43\x08\x83\xe0\xfe\x89\x43\x08\xb9\xa0\x86\x01\x00')
 c.label('rst0'); c.emit(b'\x8b\x43\x08\xa8\x01'); c.rel32(b'\x0f\x84','rst0ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','rst0'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('rst0ok'); c.emit(b'\xb9\x64\x00\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x8b\x43\x08\x83\xc8\x01\x89\x43\x08\xb9\xa0\x86\x01\x00')
 c.label('rst1'); c.emit(b'\x8b\x43\x08\xa8\x01'); c.rel32(b'\x0f\x85','rst1ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','rst1'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('rst1ok'); c.emit(b'\xb9\xe8\x03\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x0f\xb7\x43\x0e\x66\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x45\x31\xe4')
 c.label('cad'); c.emit(b'\xa8\x01'); c.rel32(b'\x0f\x85','cadok')
 c.emit(b'\x66\xd1\xe8\x41\xff\xc4\x41\x83\xfc\x0f'); c.rel32(b'\x0f\x82','cad')
 c.rel32(b'\xe9','fail_bad_hda')
 c.label('cadok'); serial('hda')

 verb(0,0x000f0000)
 c.emit(b'\x89\xc1\xc1\xe9\x10\x66\x81\xf9\xf4\x1a'); c.rel32(b'\x0f\x85','fail_vendor')
 serial('codec')
 verb(3,0x00070740)
 verb(2,0x00020011)
 verb(2,0x00070610)
 serial('route')

 c.emit(b'\x0f\xb7\x03\xc1\xe8\x08\x83\xe0\x0f\xc1\xe0\x05\x48\x8d\xbc\x03\x80\x00\x00\x00')
 c.emit(b'\x8b\x07\x83\xe0\xfd\x89\x07')
 c.emit(b'\x8b\x07\x83\xc8\x01\x89\x07\xb9\xa0\x86\x01\x00')
 c.label('sdr1'); c.emit(b'\x8b\x07\xa8\x01'); c.rel32(b'\x0f\x85','sdr1ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sdr1'); c.rel32(b'\xe9','fail_stream_reset')
 c.label('sdr1ok')
 c.emit(b'\x8b\x07\x83\xe0\xfe\x89\x07\xb9\xa0\x86\x01\x00')
 c.label('sdr0'); c.emit(b'\x8b\x07\xa8\x01'); c.rel32(b'\x0f\x84','sdr0ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sdr0'); c.rel32(b'\xe9','fail_stream_reset')
 c.label('sdr0ok')

 c.lea_rax(wave); c.lea_rdx(bdl)
 c.emit(b'\x48\x89\x02\xc7\x42\x08'+struct.pack('<I',PCM_HALF)+b'\xc7\x42\x0c\x00\x00\x00\x00')
 c.emit(b'\x48\x05'+struct.pack('<I',PCM_HALF)+b'\x48\x89\x42\x10')
 c.emit(b'\xc7\x42\x18'+struct.pack('<I',PCM_HALF)+b'\xc7\x42\x1c\x00\x00\x00\x00\x0f\x09')
 c.lea_rax(bdl)
 c.emit(b'\x89\x47\x18\x48\x89\xc2\x48\xc1\xea\x20\x89\x57\x1c')
 c.emit(b'\xc7\x47\x08'+struct.pack('<I',PCM_BYTES)+b'\x66\xc7\x47\x0c\x01\x00')
 c.emit(b'\x66\xc7\x47\x12'+struct.pack('<H',FMT))
 serial('bdl')
 c.emit(b'\xc7\x07\x02\x00\x10\x00'); serial('run')
 c.emit(b'\xb9\x60\xe3\x16\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x8b\x47\x04\x85\xc0'); c.rel32(b'\x0f\x84','fail_dma_no_progress')
 serial('progress')
 c.emit(b'\x8b\x07\x83\xe0\xfd\x89\x07'); serial('done')
 c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')

 c.label('fail_no_hda'); serial('no_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_bad_hda'); serial('bad_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_verb_timeout'); serial('verb_timeout'); c.rel32(b'\xe9','return_fail')
 c.label('fail_vendor'); serial('vendor_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_stream_reset'); serial('stream_reset_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_dma_no_progress'); c.emit(b'\x8b\x07\x83\xe0\xfd\x89\x07'); serial('dma_no_progress')
 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return'); c.emit(b'\x48\x83\xc4\x20\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5b\xc3')

 c.label('immediate')
 c.emit(b'\x41\x89\xc0\xb9\xa0\x86\x01\x00')
 c.label('ic0'); c.emit(b'\x0f\xb7\x53\x68\xf6\xc2\x01'); c.rel32(b'\x0f\x84','icgo')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','ic0'); c.rel32(b'\xe9','icto')
 c.label('icgo')
 c.emit(b'\x66\xc7\x43\x68\x02\x00\x44\x89\x43\x60\x66\xc7\x43\x68\x01\x00\xb9\xa0\x86\x01\x00')
 c.label('irv'); c.emit(b'\x0f\xb7\x53\x68\xf6\xc2\x02'); c.rel32(b'\x0f\x85','irvok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','irv')
 c.emit(b'\x66\x83\x63\x68\xfe')
 c.label('icto'); c.emit(b'\xb8\xff\xff\xff\xff\xc3')
 c.label('irvok'); c.emit(b'\x8b\x43\x64\x66\xc7\x43\x68\x02\x00\xc3')

 c.label('pci_read32'); c.emit(b'\x66\xba\xf8\x0c\xef\x66\xba\xfc\x0c\xed\xc3')
 c.label('pci_write32'); c.emit(b'\x66\xba\xf8\x0c\xef\x89\xc8\x66\xba\xfc\x0c\xef\xc3')
 c.label('serial_emit')
 c.emit(b'\x49\x89\xd0\x66\xba\xfd\x03')
 c.label('sw'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'sw')
 c.emit(b'\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9'); c.rel8(0x75,'sw'); c.emit(b'\xc3')
 c.patch(); code=bytes(c.data)
 if len(code)>=0x1000: raise SystemExit('code too large')

 text_raw=0x200; text_raw_size=(len(code)+0x1ff)&~0x1ff
 data_raw=text_raw+text_raw_size; data_raw_size=(len(data)+0x1ff)&~0x1ff
 reloc_rva=(DATA_RVA+len(data)+0xfff)&~0xfff; reloc_raw=data_raw+data_raw_size
 image=bytearray(reloc_raw+0x200); image_size=reloc_rva+0x1000
 put(image,0,'<H',0x5a4d); put(image,0x3c,'<I',0x80)
 pe=0x80; image[pe:pe+4]=b'PE\0\0'; coff=pe+4
 put(image,coff,'<HHIIIHH',0x8664,3,0,0,0,0xf0,0x22); opt=coff+20
 put(image,opt,'<H',0x20b); put(image,opt+4,'<I',text_raw_size); put(image,opt+8,'<I',data_raw_size+0x200)
 put(image,opt+0x10,'<I',TEXT_RVA); put(image,opt+0x14,'<I',TEXT_RVA); put(image,opt+0x18,'<Q',0x400000)
 put(image,opt+0x20,'<I',0x1000); put(image,opt+0x24,'<I',0x200); put(image,opt+0x38,'<I',image_size); put(image,opt+0x3c,'<I',0x200)
 put(image,opt+0x44,'<H',10); put(image,opt+0x48,'<Q',0x100000); put(image,opt+0x50,'<Q',0x1000); put(image,opt+0x58,'<Q',0x100000); put(image,opt+0x60,'<Q',0x1000); put(image,opt+0x6c,'<I',16); put(image,opt+0x70+5*8,'<II',reloc_rva,8)
 sec=opt+0xf0; image[sec:sec+8]=b'.text\0\0\0'; put(image,sec+8,'<I',len(code)); put(image,sec+0xc,'<I',TEXT_RVA); put(image,sec+0x10,'<I',text_raw_size); put(image,sec+0x14,'<I',text_raw); put(image,sec+0x24,'<I',0x60000020)
 ds=sec+40; image[ds:ds+8]=b'.data\0\0\0'; put(image,ds+8,'<I',len(data)); put(image,ds+0xc,'<I',DATA_RVA); put(image,ds+0x10,'<I',data_raw_size); put(image,ds+0x14,'<I',data_raw); put(image,ds+0x24,'<I',0xc0000040)
 rs=sec+80; image[rs:rs+8]=b'.reloc\0\0'; put(image,rs+8,'<I',8); put(image,rs+0xc,'<I',reloc_rva); put(image,rs+0x10,'<I',0x200); put(image,rs+0x14,'<I',reloc_raw); put(image,rs+0x24,'<I',0x42000040)
 image[text_raw:text_raw+len(code)]=code; image[data_raw:data_raw+len(data)]=data; put(image,reloc_raw,'<II',TEXT_RVA,8)
 return bytes(image)

def validate(image):
 assert image[:2]==b'MZ'
 pe=struct.unpack_from('<I',image,0x3c)[0]; assert image[pe:pe+4]==b'PE\0\0'
 coff=pe+4; assert struct.unpack_from('<HH',image,coff)==(0x8664,3)
 opt=coff+20; assert struct.unpack_from('<H',image,opt)[0]==0x20b; assert struct.unpack_from('<H',image,opt+0x44)[0]==10
 sec=opt+0xf0; ds=sec+40
 tc=struct.unpack_from('<I',image,sec+0x24)[0]; dc=struct.unpack_from('<I',image,ds+0x24)[0]
 assert not(tc&0x80000000); assert dc&0x80000000 and not(dc&0x20000000)
 for m in MARKS.values(): assert m in image

def main():
 if len(sys.argv)!=2: raise SystemExit('usage: build_uefi_hda_pcm.py OUTPUT_EFI')
 image=build(); validate(image)
 p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HDA_PCM_BUILD=PASS')
 print('pcm-bytes='+str(PCM_BYTES))
 print('bytes='+str(len(image)))
 print('sha256='+hashlib.sha256(image).hexdigest())
if __name__=='__main__': main()
