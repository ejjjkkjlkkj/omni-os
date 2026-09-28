#!/usr/bin/env python3
from __future__ import annotations
import hashlib, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x2000

MARKS={
 'start': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATE=START\r\nTRANSPORT=IMMEDIATE_COMMAND\r\nEND\r\n',
 'hda': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nHDA_CONTROLLER_CODEC=PASS\r\nEND\r\n',
 'vendor': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nGET_PARAMETER_VENDOR_ID=PASS\r\nQEMU_CODEC_VENDOR_1AF4=PASS\r\nEND\r\n',
 'nodes': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nGET_PARAMETER_SUBORDINATE_NODES=PASS\r\nEND\r\n',
 'afg': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nGET_PARAMETER_FUNCTION_GROUP_TYPE=PASS\r\nAUDIO_FUNCTION_GROUP=PASS\r\nEND\r\n',
 'widgets': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nDAC_WIDGET=PASS\r\nOUTPUT_PIN_WIDGET=PASS\r\nPIN_OUTPUT_CAP=PASS\r\nEND\r\n',
 'route': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nPIN_CONNECTION_LIST=PASS\r\nPIN_TO_DAC_ROUTE=PASS\r\nEND\r\n',
 'done': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=PASS\r\nEND\r\n',
 'no_hda': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n',
 'bad_hda': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CONTROLLER_OR_CODEC_FAILED\r\nEND\r\n',
 'verb_timeout': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=IMMEDIATE_COMMAND_TIMEOUT\r\nEND\r\n',
 'vendor_fail': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=CODEC_VENDOR_UNEXPECTED\r\nEND\r\n',
 'nodes_fail': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=SUBORDINATE_NODES_INVALID\r\nEND\r\n',
 'afg_fail': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=AUDIO_FUNCTION_GROUP_NOT_FOUND\r\nEND\r\n',
 'widget_fail': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=OUTPUT_WIDGET_TOPOLOGY_INVALID\r\nEND\r\n',
 'route_fail': b'QEVARYNOX-UEFI-HDA-WIDGET-V1\r\nSTATUS=BLOCKED\r\nREASON=OUTPUT_PIN_ROUTE_INVALID\r\nEND\r\n',
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
 def data_disp(self,opcode,off):
  self.emit(opcode); after=TEXT_RVA+self.pos()+4; self.emit(struct.pack('<i',DATA_RVA+off-after))
 def lea_rdx_data(self,off): self.data_disp(b'\x48\x8d\x15',off)
 def patch(self):
  for off,after,label,width in self.fix:
   disp=self.labels[label]-after
   if width==4: struct.pack_into('<i',self.data,off,disp)
   else:
    if not -128<=disp<=127: raise SystemExit(f'short overflow {label}: {disp}')
    self.data[off]=disp&0xff

def put(b,o,f,*v): struct.pack_into(f,b,o,*v)

def build():
 data=bytearray(); L={}
 for n,m in MARKS.items(): L[n]=len(data); data+=m
 c=Code()
 c.emit(b'\x53\x41\x54\x41\x55\x41\x56\x41\x57')
 c.emit(b'\x4c\x8b\x7a\x60')
 c.emit(b'\x48\x83\xec\x20\xfc')
 for p,v in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
  c.emit(b'\x66\xba'+struct.pack('<H',p)+b'\xb0'+bytes((v,))+b'\xee')
 def serial(n):
  c.lea_rdx_data(L[n]); c.emit(b'\xb9'+struct.pack('<I',len(MARKS[n]))); c.rel32(b'\xe8','serial_emit')
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
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_bad_hda')
 c.label('bar_low')
 c.emit(b'\x44\x89\xf0\x25\xf0\xff\xff\xff\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x48\x89\xc3')
 c.emit(b'\x0f\xb7\x03\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x0f\xb6\x43\x03\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x8b\x43\x08\x83\xe0\xfe\x89\x43\x08\xb9\xa0\x86\x01\x00')
 c.label('reset_clear_poll'); c.emit(b'\x8b\x43\x08\xa8\x01'); c.rel32(b'\x0f\x84','reset_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','reset_clear_poll'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('reset_clear_ok')
 c.emit(b'\xb9\x64\x00\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x8b\x43\x08\x83\xc8\x01\x89\x43\x08\xb9\xa0\x86\x01\x00')
 c.label('reset_set_poll'); c.emit(b'\x8b\x43\x08\xa8\x01'); c.rel32(b'\x0f\x85','reset_set_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','reset_set_poll'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('reset_set_ok')
 c.emit(b'\xb9\xe8\x03\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x0f\xb7\x43\x0e\x66\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x45\x31\xe4')
 c.label('cad_loop')
 c.emit(b'\xa8\x01'); c.rel32(b'\x0f\x85','cad_found')
 c.emit(b'\x66\xd1\xe8\x41\xff\xc4\x41\x83\xfc\x0f'); c.rel32(b'\x0f\x82','cad_loop')
 c.rel32(b'\xe9','fail_bad_hda')
 c.label('cad_found')
 serial('hda')

 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x00\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\x89\xc1\xc1\xe9\x10\x66\x81\xf9\xf4\x1a'); c.rel32(b'\x0f\x85','fail_vendor')
 serial('vendor')

 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x04\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\x41\x89\xc5\x41\x81\xe5\xff\x00\x00\x00\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_nodes')
 c.emit(b'\x89\xc6\xc1\xee\x10\x81\xe6\xff\x00\x00\x00\x85\xf6'); c.rel32(b'\x0f\x84','fail_nodes')
 serial('nodes')

 c.label('afg_loop')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c')
 c.emit(b'\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x05\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x01'); c.rel32(b'\x0f\x84','afg_found')
 c.emit(b'\xff\xc6\x41\xff\xcd'); c.rel32(b'\x0f\x85','afg_loop')
 c.rel32(b'\xe9','fail_afg')
 c.label('afg_found')
 serial('afg')

 # Query QEMU witness DAC NID 2 Audio Widget Capabilities (param 09h).
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x09\x00\x2f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\x89\xc1\xc1\xe9\x14\x83\xe1\x0f\x85\xc9'); c.rel32(b'\x0f\x85','fail_widget')

 # Query QEMU witness output pin NID 3 and require widget type 4.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x09\x00\x3f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\x89\xc1\xc1\xe9\x14\x83\xe1\x0f\x83\xf9\x04'); c.rel32(b'\x0f\x85','fail_widget')

 # Pin Capabilities param 0Ch: output capable bit 4 must be set.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x0c\x00\x3f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\xa8\x10'); c.rel32(b'\x0f\x84','fail_widget')
 serial('widgets')

 # Connection-list length param 0Eh must contain at least one entry.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x0e\x00\x3f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\xa8\x7f'); c.rel32(b'\x0f\x84','fail_route')

 # GET_CONNECT_LIST F02h, index 0. First short-form entry must route pin 3 -> DAC 2.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x00\x02\x3f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb_timeout')
 c.emit(b'\x3c\x02'); c.rel32(b'\x0f\x85','fail_route')
 serial('route'); serial('done')
 c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')

 c.label('fail_no_hda'); serial('no_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_bad_hda'); serial('bad_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_verb_timeout'); serial('verb_timeout'); c.rel32(b'\xe9','return_fail')
 c.label('fail_vendor'); serial('vendor_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_nodes'); serial('nodes_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_afg'); serial('afg_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_widget'); serial('widget_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_route'); serial('route_fail')
 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return'); c.emit(b'\x48\x83\xc4\x20\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5b\xc3')

 c.label('immediate')
 c.emit(b'\x41\x89\xc0')
 c.emit(b'\xb9\xa0\x86\x01\x00')
 c.label('ic_ready_poll')
 c.emit(b'\x0f\xb7\x53\x68\xf6\xc2\x01')
 c.rel32(b'\x0f\x84','ic_ready')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','ic_ready_poll'); c.rel32(b'\xe9','ic_timeout')
 c.label('ic_ready')
 c.emit(b'\x66\xc7\x43\x68\x02\x00')
 c.emit(b'\x44\x89\x43\x60')
 c.emit(b'\x66\xc7\x43\x68\x01\x00')
 c.emit(b'\xb9\xa0\x86\x01\x00')
 c.label('irv_poll')
 c.emit(b'\x0f\xb7\x53\x68\xf6\xc2\x02')
 c.rel32(b'\x0f\x85','irv_ready')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','irv_poll')
 c.emit(b'\x66\x83\x63\x68\xfe')
 c.label('ic_timeout'); c.emit(b'\xb8\xff\xff\xff\xff\xc3')
 c.label('irv_ready')
 c.emit(b'\x8b\x43\x64')
 c.emit(b'\x66\xc7\x43\x68\x02\x00')
 c.emit(b'\xc3')

 c.label('pci_read32'); c.emit(b'\x66\xba\xf8\x0c\xef\x66\xba\xfc\x0c\xed\xc3')
 c.label('pci_write32'); c.emit(b'\x66\xba\xf8\x0c\xef\x89\xc8\x66\xba\xfc\x0c\xef\xc3')
 c.label('serial_emit')
 c.emit(b'\x49\x89\xd0\x66\xba\xfd\x03')
 c.label('serial_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'serial_wait')
 c.emit(b'\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9'); c.rel8(0x75,'serial_wait'); c.emit(b'\xc3')
 c.patch(); code=bytes(c.data)

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
 assert image[:2]==b'MZ'; pe=struct.unpack_from('<I',image,0x3c)[0]; assert image[pe:pe+4]==b'PE\0\0'
 coff=pe+4; assert struct.unpack_from('<HH',image,coff)==(0x8664,3); opt=coff+20; assert struct.unpack_from('<H',image,opt)[0]==0x20b; assert struct.unpack_from('<H',image,opt+0x44)[0]==10
 sec=opt+0xf0; ds=sec+40; tc=struct.unpack_from('<I',image,sec+0x24)[0]; dc=struct.unpack_from('<I',image,ds+0x24)[0]; assert not(tc&0x80000000); assert dc&0x80000000 and not(dc&0x20000000)
 for m in MARKS.values(): assert m in image

def main():
 if len(sys.argv)!=2: raise SystemExit('usage: build_uefi_hda_widget_probe.py OUTPUT_EFI')
 image=build(); validate(image); p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HDA_WIDGET_PROBE_BUILD=PASS'); print('bytes='+str(len(image))); print('sha256='+hashlib.sha256(image).hexdigest())
if __name__=='__main__': main()
