#!/usr/bin/env python3
from __future__ import annotations
import hashlib, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x4000

MARKS={
 'start': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATE=START\r\nDOMAIN=PRE_OS_UEFI\r\nEND\r\n',
 'database': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nHII_DATABASE_PROTOCOL=PASS\r\nEND\r\n',
 'export': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nHII_EXPORT_ALL_PACKAGE_LISTS=PASS\r\nEND\r\n',
 'lists': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nHII_PACKAGE_LIST_NONZERO=PASS\r\nEND\r\n',
 'forms': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nHII_FORMS_PACKAGE=PASS\r\nEND\r\n',
 'strings': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nHII_STRINGS_PACKAGE=PASS\r\nEND\r\n',
 'done': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=PASS\r\nEND\r\n',
 'no_db': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_DATABASE_PROTOCOL_NOT_FOUND\r\nEND\r\n',
 'size_fail': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_EXPORT_SIZE_QUERY_FAILED\r\nEND\r\n',
 'alloc_fail': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_EXPORT_ALLOC_FAILED\r\nEND\r\n',
 'export_fail': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_EXPORT_FAILED\r\nEND\r\n',
 'parse_fail': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_PACKAGE_PARSE_FAILED\r\nEND\r\n',
 'no_forms': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_FORMS_PACKAGE_NOT_FOUND\r\nEND\r\n',
 'no_strings': b'QEVARYNOX-UEFI-HII-PROBE-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_STRINGS_PACKAGE_NOT_FOUND\r\nEND\r\n',
}

class Code:
 def __init__(self):
  self.data=bytearray(); self.labels={}; self.fix=[]
 def pos(self): return len(self.data)
 def emit(self,b): self.data += bytes(b)
 def label(self,n): self.labels[n]=self.pos()
 def rel32(self,op,label):
  self.emit(op); off=self.pos(); self.emit(b'\0'*4); self.fix.append((off,self.pos(),label,4))
 def rel8(self,op,label):
  self.emit(bytes((op,0))); self.fix.append((self.pos()-1,self.pos(),label,1))
 def data_disp(self,opcode,off):
  self.emit(opcode); after=TEXT_RVA+self.pos()+4
  self.emit(struct.pack('<i',DATA_RVA+off-after))
 def lea_rcx_data(self,off): self.data_disp(b'\x48\x8d\x0d',off)
 def lea_rdx_data(self,off): self.data_disp(b'\x48\x8d\x15',off)
 def lea_r8_data(self,off): self.data_disp(b'\x4c\x8d\x05',off)
 def patch(self):
  for off,after,label,width in self.fix:
   disp=self.labels[label]-after
   if width==4: struct.pack_into('<i',self.data,off,disp)
   else:
    if not -128 <= disp <= 127: raise SystemExit(f'short jump overflow: {label} {disp}')
    self.data[off]=disp & 0xff

def put(b,o,f,*v): struct.pack_into(f,b,o,*v)

def build():
 data=bytearray(0x80)
 L={'guid':0,'dbptr':16,'size':24,'bufptr':32,'lists':40,'forms':44,'strings':48}
 # EFI_HII_DATABASE_PROTOCOL_GUID:
 # ef9fc172-a1b2-4693-b327-6d32fc416042
 struct.pack_into('<IHH8B',data,L['guid'],
  0xef9fc172,0xa1b2,0x4693,0xb3,0x27,0x6d,0x32,0xfc,0x41,0x60,0x42)
 for n,m in MARKS.items():
  L[n]=len(data); data+=m

 c=Code()
 # Preserve nonvolatile registers. UEFI x86-64 uses the Microsoft ABI.
 c.emit(b'\x53\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57')
 # r15 = EFI_BOOT_SERVICES from EFI_SYSTEM_TABLE + 0x60.
 c.emit(b'\x4c\x8b\x7a\x60')
 c.emit(b'\x48\x83\xec\x20\xfc')

 # Initialize COM1 for machine-readable witness output.
 for p,v in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
  c.emit(b'\x66\xba'+struct.pack('<H',p)+b'\xb0'+bytes((v,))+b'\xee')

 def serial(n):
  c.lea_rdx_data(L[n])
  c.emit(b'\xb9'+struct.pack('<I',len(MARKS[n])))
  c.rel32(b'\xe8','serial_emit')

 serial('start')

 # LocateProtocol(&gEfiHiiDatabaseProtocolGuid, NULL, &dbptr)
 c.lea_rcx_data(L['guid'])
 c.emit(b'\x31\xd2')
 c.lea_r8_data(L['dbptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')  # BootServices->LocateProtocol +0x140
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_no_db')
 c.lea_rdx_data(L['dbptr'])
 c.emit(b'\x4c\x8b\x22')  # r12 = HII database protocol
 c.emit(b'\x4d\x85\xe4'); c.rel32(b'\x0f\x84','fail_no_db')
 serial('database')

 # Query required size: ExportPackageLists(This, NULL, &size, NULL)
 c.emit(b'\x4c\x89\xe1\x31\xd2')
 c.lea_r8_data(L['size'])
 c.emit(b'\x45\x31\xc9')
 c.emit(b'\x41\xff\x54\x24\x20')  # HII DB method slot 4
 # The first call is expected to report BUFFER_TOO_SMALL; size is authoritative.
 c.lea_rdx_data(L['size'])
 c.emit(b'\x48\x8b\x1a')
 c.emit(b'\x48\x83\xfb\x18'); c.rel32(b'\x0f\x82','fail_size')

 # AllocatePool(EfiBootServicesData=4, size, &bufptr)
 c.emit(b'\xb9\x04\x00\x00\x00')
 c.emit(b'\x48\x89\xda')
 c.lea_r8_data(L['bufptr'])
 c.emit(b'\x41\xff\x57\x40')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_alloc')
 c.lea_rdx_data(L['bufptr'])
 c.emit(b'\x4c\x8b\x2a')  # r13 = exported buffer
 c.emit(b'\x4d\x85\xed'); c.rel32(b'\x0f\x84','fail_alloc')

 # Export all package lists into allocated buffer.
 c.emit(b'\x4c\x89\xe1\x31\xd2')
 c.lea_r8_data(L['size'])
 c.emit(b'\x4d\x89\xe9')
 c.emit(b'\x41\xff\x54\x24\x20')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_export')
 serial('export')

 # Parse concatenated EFI_HII_PACKAGE_LIST_HEADER records.
 # rsi=current list; rbx=remaining exported bytes.
 c.emit(b'\x4c\x89\xee')  # rsi=r13
 c.lea_rdx_data(L['size']); c.emit(b'\x48\x8b\x1a')
 c.label('list_loop')
 c.emit(b'\x48\x83\xfb\x14'); c.rel32(b'\x0f\x82','fail_parse')
 c.emit(b'\x8b\x46\x10')      # eax = PackageLength
 c.emit(b'\x83\xf8\x18'); c.rel32(b'\x0f\x82','fail_parse')
 c.emit(b'\x48\x39\xd8'); c.rel32(b'\x0f\x87','fail_parse')
 c.emit(b'\x41\x89\xc6')      # r14d = list length
 # ++list counter
 c.lea_rdx_data(L['lists']); c.emit(b'\xff\x02')
 # rdi = first package, ecx = list package bytes
 c.emit(b'\x48\x8d\x7e\x14')
 c.emit(b'\x44\x89\xf1\x83\xe9\x14')
 c.label('package_loop')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x82','fail_parse')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00') # edx=length
 c.emit(b'\x89\xc5\xc1\xed\x18') # ebp=type
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x82','fail_parse')
 c.emit(b'\x39\xca'); c.rel32(b'\x0f\x87','fail_parse')
 c.emit(b'\x81\xfd\xdf\x00\x00\x00'); c.rel32(b'\x0f\x84','package_end')
 c.emit(b'\x83\xfd\x02'); c.rel32(b'\x0f\x85','not_forms')
 c.lea_rcx_data(L['forms']); c.emit(b'\xff\x01')
 c.label('not_forms')
 c.emit(b'\x83\xfd\x04'); c.rel32(b'\x0f\x85','not_strings')
 c.lea_rcx_data(L['strings']); c.emit(b'\xff\x01')
 c.label('not_strings')
 c.emit(b'\x48\x01\xd7\x29\xd1')
 c.rel32(b'\xe9','package_loop')

 c.label('package_end')
 # END package must fit, then advance exactly PackageLength bytes.
 c.emit(b'\x44\x89\xf0')
 c.emit(b'\x48\x01\xc6')
 c.emit(b'\x48\x29\xc3')
 c.emit(b'\x48\x85\xdb'); c.rel32(b'\x0f\x85','list_loop')

 # Require at least one list, one Forms package and one Strings package.
 c.lea_rdx_data(L['lists']); c.emit(b'\x83\x3a\x00'); c.rel32(b'\x0f\x84','fail_parse')
 serial('lists')
 c.lea_rdx_data(L['forms']); c.emit(b'\x83\x3a\x00'); c.rel32(b'\x0f\x84','fail_no_forms')
 serial('forms')
 c.lea_rdx_data(L['strings']); c.emit(b'\x83\x3a\x00'); c.rel32(b'\x0f\x84','fail_no_strings')
 serial('strings')
 serial('done')
 c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')

 c.label('fail_no_db'); serial('no_db'); c.rel32(b'\xe9','return_fail')
 c.label('fail_size'); serial('size_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_alloc'); serial('alloc_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_export'); serial('export_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_parse'); serial('parse_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_no_forms'); serial('no_forms'); c.rel32(b'\xe9','return_fail')
 c.label('fail_no_strings'); serial('no_strings')
 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return')
 c.emit(b'\x48\x83\xc4\x20\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5f\x5e\x5d\x5b\xc3')

 c.label('serial_emit')
 c.emit(b'\x49\x89\xd0\x66\xba\xfd\x03')
 c.label('serial_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'serial_wait')
 c.emit(b'\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9')
 c.rel8(0x75,'serial_wait'); c.emit(b'\xc3')

 c.patch(); code=bytes(c.data)
 if len(code)>0x2800: raise SystemExit(f'HII probe code too large: {len(code)}')

 text_raw=0x200; text_raw_size=(len(code)+0x1ff)&~0x1ff
 data_raw=text_raw+text_raw_size; data_raw_size=(len(data)+0x1ff)&~0x1ff
 reloc_rva=(DATA_RVA+len(data)+0xfff)&~0xfff
 reloc_raw=data_raw+data_raw_size
 image=bytearray(reloc_raw+0x200)
 image_size=reloc_rva+0x1000

 put(image,0,'<H',0x5a4d); put(image,0x3c,'<I',0x80)
 pe=0x80; image[pe:pe+4]=b'PE\0\0'; coff=pe+4
 put(image,coff,'<HHIIIHH',0x8664,3,0,0,0,0xf0,0x22); opt=coff+20
 put(image,opt,'<H',0x20b)
 put(image,opt+4,'<I',text_raw_size)
 put(image,opt+8,'<I',data_raw_size+0x200)
 put(image,opt+0x10,'<I',TEXT_RVA); put(image,opt+0x14,'<I',TEXT_RVA)
 put(image,opt+0x18,'<Q',0x400000)
 put(image,opt+0x20,'<I',0x1000); put(image,opt+0x24,'<I',0x200)
 put(image,opt+0x38,'<I',image_size); put(image,opt+0x3c,'<I',0x200)
 put(image,opt+0x44,'<H',10)
 put(image,opt+0x48,'<Q',0x100000); put(image,opt+0x50,'<Q',0x1000)
 put(image,opt+0x58,'<Q',0x100000); put(image,opt+0x60,'<Q',0x1000)
 put(image,opt+0x6c,'<I',16)
 put(image,opt+0x70+5*8,'<II',reloc_rva,8)

 sec=opt+0xf0
 image[sec:sec+8]=b'.text\0\0\0'
 put(image,sec+8,'<I',len(code)); put(image,sec+0xc,'<I',TEXT_RVA)
 put(image,sec+0x10,'<I',text_raw_size); put(image,sec+0x14,'<I',text_raw)
 put(image,sec+0x24,'<I',0x60000020)
 ds=sec+40
 image[ds:ds+8]=b'.data\0\0\0'
 put(image,ds+8,'<I',len(data)); put(image,ds+0xc,'<I',DATA_RVA)
 put(image,ds+0x10,'<I',data_raw_size); put(image,ds+0x14,'<I',data_raw)
 put(image,ds+0x24,'<I',0xc0000040)
 rs=sec+80
 image[rs:rs+8]=b'.reloc\0\0'
 put(image,rs+8,'<I',8); put(image,rs+0xc,'<I',reloc_rva)
 put(image,rs+0x10,'<I',0x200); put(image,rs+0x14,'<I',reloc_raw)
 put(image,rs+0x24,'<I',0x42000040)

 image[text_raw:text_raw+len(code)]=code
 image[data_raw:data_raw+len(data)]=data
 put(image,reloc_raw,'<II',TEXT_RVA,8)
 return bytes(image)

def validate(image):
 assert image[:2]==b'MZ'
 pe=struct.unpack_from('<I',image,0x3c)[0]
 assert image[pe:pe+4]==b'PE\0\0'
 for token in (
  b'QEVARYNOX-UEFI-HII-PROBE-V1',
  b'HII_DATABASE_PROTOCOL=PASS',
  b'HII_EXPORT_ALL_PACKAGE_LISTS=PASS',
  b'HII_FORMS_PACKAGE=PASS',
  b'HII_STRINGS_PACKAGE=PASS',
 ):
  assert token in image,token

def main():
 if len(sys.argv)!=2:
  raise SystemExit('usage: build_uefi_hii_probe.py OUTPUT_EFI')
 image=build(); validate(image)
 p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HII_PROBE_BUILD=PASS')
 print('bytes='+str(len(image)))
 print('sha256='+hashlib.sha256(image).hexdigest())

if __name__=='__main__':
 main()
