#!/usr/bin/env python3
from __future__ import annotations
import hashlib, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x5000

MARKS={
 'start': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATE=START\r\nDOMAIN=PRE_OS_UEFI\r\nEND\r\n',
 'database': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_DATABASE_PROTOCOL=PASS\r\nEND\r\n',
 'string_protocol': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_STRING_PROTOCOL=PASS\r\nEND\r\n',
 'direct_export': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_EXPORT_ALL_PACKAGE_LISTS=PASS\r\nEND\r\n',
 'forms_handle': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_FORMS_HANDLE=LEGACY_NOT_USED\r\nEND\r\n',
 'strings_package': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_STRINGS_PACKAGE=PASS\r\nEND\r\n',
 'strings_retry': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_STRINGS_PACKAGE_RETRY=PASS\r\nEND\r\n',
 'first_handle': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_FIRST_HANDLE_NONZERO=PASS\r\nEND\r\n',
 'static_empty': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_BUFFER_NOT_WRITTEN\r\nEND\r\n',
 'handle_export': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_HANDLE_EXPORT=PASS\r\nEND\r\n',
 'export_size_ok': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_HANDLE_EXPORT_SIZE=PASS\r\nEND\r\n',
 'export_size_invalid': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_HANDLE_EXPORT_SIZE=EFI_INVALID_PARAMETER\r\nEND\r\n',
 'export_size_not_found': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_HANDLE_EXPORT_SIZE=EFI_NOT_FOUND\r\nEND\r\n',
 'export_fetch_invalid': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_HANDLE_EXPORT_FETCH=EFI_INVALID_PARAMETER\r\nEND\r\n',
 'export_fetch_not_found': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_HANDLE_EXPORT_FETCH=EFI_NOT_FOUND\r\nEND\r\n',
 'forms_package_seen': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_FORMS_PACKAGE=PASS\r\nEND\r\n',
 'ifr': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nIFR_QUESTION_PROMPT_STRING_ID=PASS\r\nEND\r\n',
 'language': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nHII_LANGUAGE=PASS\r\nEND\r\n',
 'prefix': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nQUESTION_TEXT=',
 'suffix': b'\r\nHII_QUESTION_STRING=PASS\r\nSTATUS=PASS\r\nEND\r\n',
 'meta_qid': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nQUESTION_ID_LE_HEX=',
 'meta_varstore': b'\r\nVARSTORE_ID_LE_HEX=',
 'meta_varinfo': b'\r\nVARSTORE_INFO_LE_HEX=',
 'meta_qflags': b'\r\nQUESTION_FLAGS_HEX=',
 'meta_oneof': b'\r\nONEOF_FLAGS_HEX=',
 'meta_done': b'\r\nQUESTION_METADATA=PASS\r\nNONZERO_VARSTORE_ID=PASS\r\nEND\r\n',
 'no_protocol': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_PROTOCOL_NOT_FOUND\r\nEND\r\n',
 'list_size_fail': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_SIZE_QUERY_FAILED\r\nEND\r\n',
 'list_fetch_fail': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_FAILED\r\nEND\r\n',
 'list_fetch_invalid': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_EFI_INVALID_PARAMETER\r\nEND\r\n',
 'list_fetch_not_found': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_EFI_NOT_FOUND\r\nEND\r\n',
 'list_fetch_bts_twice': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_BUFFER_TOO_SMALL_TWICE\r\nEND\r\n',
 'list_static_small': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_STATIC_BUFFER_TOO_SMALL\r\nEND\r\n',
 'no_handle': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_FORMS_AND_STRINGS_PACKAGE_LIST_NOT_FOUND\r\nEND\r\n',
 'alloc': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=POOL_ALLOC_FAILED\r\nEND\r\n',
 'export': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_HANDLE_EXPORT_FAILED\r\nEND\r\n',
 'ifr_fail': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=IFR_QUESTION_TOKEN_NOT_FOUND\r\nEND\r\n',
 'lang_fail': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LANGUAGE_NOT_FOUND\r\nEND\r\n',
 'string_fail': b'QEVARYNOX-UEFI-HII-STORED-QUESTION-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_QUESTION_STRING_NOT_RESOLVED\r\nEND\r\n',
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
 def lea_rax_data(self,off): self.data_disp(b'\x48\x8d\x05',off)
 def lea_rcx_data(self,off): self.data_disp(b'\x48\x8d\x0d',off)
 def lea_rdx_data(self,off): self.data_disp(b'\x48\x8d\x15',off)
 def lea_rsi_data(self,off): self.data_disp(b'\x48\x8d\x35',off)
 def lea_r8_data(self,off): self.data_disp(b'\x4c\x8d\x05',off)
 def lea_r9_data(self,off): self.data_disp(b'\x4c\x8d\x0d',off)
 def patch(self):
  for off,after,label,width in self.fix:
   disp=self.labels[label]-after
   if width==4: struct.pack_into('<i',self.data,off,disp)
   else:
    if not -128 <= disp <= 127: raise SystemExit(f'short jump overflow {label}: {disp}')
    self.data[off]=disp & 0xff

def put(b,o,f,*v): struct.pack_into(f,b,o,*v)

def build():
 data=bytearray(0x101100)
 L={
  'db_guid':0,'str_guid':16,'dbptr':32,'strptr':40,
  'handles_size':48,'handles_ptr':56,'pkg_size':64,'pkg_ptr':72,
  'langs_size':80,'langs_ptr':88,'string_size':96,'string_ptr':104,
  'token':112,'temp_handle':120,'handle_cursor':128,'handles_remaining':136,
  'forms_ptr':144,'strings_ptr':152,'list_len':160,'ifr_next_ptr':168,'ifr_next_remaining':176,
  'strings_first':184,'list_start':192,'global_remaining':200,
  'question_id':208,'varstore_id':210,'varstore_info':212,'question_flags':214,'oneof_flags':215,
  'handles_static':0x100,'pkg_static':0x1100,
 }
 struct.pack_into('<IHH8B',data,L['db_guid'],
  0xef9fc172,0xa1b2,0x4693,0xb3,0x27,0x6d,0x32,0xfc,0x41,0x60,0x42)
 struct.pack_into('<IHH8B',data,L['str_guid'],
  0x0fd96974,0x23aa,0x4cdc,0xb9,0xcb,0x98,0xd1,0x77,0x50,0x32,0x2a)
 for n,m in MARKS.items():
  L[n]=len(data); data+=m

 c=Code()
 c.emit(b'\x53\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57')
 c.emit(b'\x4c\x8b\x7a\x60')  # r15=BootServices
 c.emit(b'\x48\x83\xec\x68\xfc')

 for p,v in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
  c.emit(b'\x66\xba'+struct.pack('<H',p)+b'\xb0'+bytes((v,))+b'\xee')

 def serial(n):
  c.lea_rdx_data(L[n]); c.emit(b'\xb9'+struct.pack('<I',len(MARKS[n])))
  c.rel32(b'\xe8','serial_emit')
 def zero_qword(off):
  c.lea_rdx_data(off); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 def alloc(size_reg_is_rbx,ptr_off):
  c.emit(b'\xb9\x04\x00\x00\x00') # EfiBootServicesData
  if size_reg_is_rbx: c.emit(b'\x48\x89\xda')
  c.lea_r8_data(ptr_off)
  c.emit(b'\x41\xff\x57\x40')
  c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_alloc')

 serial('start')

 # Locate HII Database protocol -> r12.
 c.lea_rcx_data(L['db_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['dbptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_protocol')
 c.lea_rdx_data(L['dbptr']); c.emit(b'\x4c\x8b\x22')
 c.emit(b'\x4d\x85\xe4'); c.rel32(b'\x0f\x84','fail_protocol')
 serial('database')

 # Locate HII String protocol -> r13.
 c.lea_rcx_data(L['str_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['strptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_protocol')
 c.lea_rdx_data(L['strptr']); c.emit(b'\x4c\x8b\x2a')
 c.emit(b'\x4d\x85\xed'); c.rel32(b'\x0f\x84','fail_protocol')
 serial('string_protocol')

 # Export the whole live HII database read-only in one atomic protocol call.
 # A bridge-owned 1 MiB buffer avoids the observed OVMF sizing/fetch race.
 c.lea_rdx_data(L['pkg_size'])
 c.emit(b'\x48\xc7\x02'+struct.pack('<I',0x100000))
 c.lea_rax_data(L['pkg_static'])
 c.lea_rdx_data(L['pkg_ptr']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x4c\x89\xe1\x31\xd2')
 c.lea_r8_data(L['pkg_size'])
 c.lea_r9_data(L['pkg_static'])
 c.emit(b'\x41\xff\x54\x24\x20')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_export')
 c.lea_rdx_data(L['pkg_size']); c.emit(b'\x48\x83\x3a\x18')
 c.rel32(b'\x0f\x82','fail_export')
 serial('direct_export')

 # Parse concatenated EFI_HII_PACKAGE_LIST_HEADER records.  Select one package
 # list containing both Forms and Strings so the IFR StringId and text share
 # the same package-list namespace.
 c.lea_rdx_data(L['pkg_ptr']); c.emit(b'\x48\x8b\x32')
 c.lea_rdx_data(L['pkg_size']); c.emit(b'\x48\x8b\x1a')
 c.label('direct_list_loop')
 c.emit(b'\x48\x83\xfb\x14'); c.rel32(b'\x0f\x82','fail_no_handle')
 c.lea_rdx_data(L['global_remaining']); c.emit(b'\x48\x89\x1a')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x89\x32')
 c.emit(b'\x8b\x46\x10\x83\xf8\x18'); c.rel32(b'\x0f\x82','fail_ifr')
 c.emit(b'\x48\x39\xd8'); c.rel32(b'\x0f\x87','fail_ifr')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x89\x02')
 zero_qword(L['forms_ptr']); zero_qword(L['strings_ptr'])
 c.emit(b'\x48\x8d\x7e\x14\x89\xc1\x83\xe9\x14')
 c.label('direct_pkg_loop')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x82','fail_ifr')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00')
 c.emit(b'\x89\xc5\xc1\xed\x18')
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x82','fail_ifr')
 c.emit(b'\x39\xca'); c.rel32(b'\x0f\x87','fail_ifr')
 c.emit(b'\x81\xfd\xdf\x00\x00\x00'); c.rel32(b'\x0f\x84','direct_list_done')
 c.emit(b'\x83\xfd\x02'); c.rel32(b'\x0f\x85','direct_not_forms')
 c.lea_rax_data(L['forms_ptr']); c.emit(b'\x48\x83\x38\x00'); c.rel32(b'\x0f\x85','direct_not_forms')
 c.emit(b'\x48\x89\x38')
 c.label('direct_not_forms')
 c.emit(b'\x83\xfd\x04'); c.rel32(b'\x0f\x85','direct_not_strings')
 c.lea_rax_data(L['strings_ptr']); c.emit(b'\x48\x83\x38\x00'); c.rel32(b'\x0f\x85','direct_not_strings')
 c.emit(b'\x48\x89\x38')
 c.label('direct_not_strings')
 c.emit(b'\x48\x01\xd7\x29\xd1'); c.rel32(b'\xe9','direct_pkg_loop')

 c.label('direct_list_done')
 c.lea_rdx_data(L['forms_ptr']); c.emit(b'\x48\x83\x3a\x00'); c.rel32(b'\x0f\x84','direct_list_next')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x83\x3a\x00'); c.rel32(b'\x0f\x84','direct_list_next')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x89\x32')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x02')
 c.lea_rdx_data(L['strings_first']); c.emit(b'\x48\x89\x02')
 serial('forms_package_seen'); serial('strings_package')
 c.lea_rdx_data(L['forms_ptr']); c.emit(b'\x48\x8b\x3a')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00')
 c.rel32(b'\xe9','forms_pkg')

 c.label('direct_list_next')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x8b\x32')
 c.lea_rdx_data(L['global_remaining']); c.emit(b'\x48\x8b\x1a')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x8b\x02')
 c.emit(b'\x48\x01\xc6\x48\x29\xc3')
 c.emit(b'\x48\x85\xdb'); c.rel32(b'\x0f\x85','direct_list_loop')
 c.rel32(b'\xe9','fail_no_handle')

 c.label('forms_pkg')
 # r9=first IFR opcode, r10d=bytes available in verified Forms package.
 c.emit(b'\x4c\x8d\x4f\x04\x41\x89\xd2\x41\x83\xea\x04')
 c.label('ifr_loop')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','prompt_package_done')
 c.emit(b'\x41\x0f\xb6\x01')       # eax=OpCode
 c.emit(b'\x41\x0f\xb6\x49\x01\x83\xe1\x7f') # ecx=Length
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','fail_ifr')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','fail_ifr')
 c.emit(b'\x3c\x05'); c.rel32(b'\x0f\x84','ifr_question')
 c.label('ifr_next')
 c.emit(b'\x49\x01\xc9\x41\x29\xca')
 c.rel32(b'\xe9','ifr_loop')

 c.label('ifr_question')
 # EFI_IFR_ONE_OF: OpHeader(2) + Statement(Prompt,Help=4) + QuestionId(2)
 # + VarStoreId(2) + VarStoreInfo(2) + QuestionFlags(1) + OneOfFlags(1).
 c.emit(b'\x83\xf9\x0e'); c.rel32(b'\x0f\x82','ifr_next')
 c.emit(b'\x41\x0f\xb7\x41\x06'); c.lea_rdx_data(L['question_id']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb7\x41\x08')
 c.emit(b'\x66\x85\xc0'); c.rel32(b'\x0f\x84','ifr_next')
 c.lea_rdx_data(L['varstore_id']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb7\x41\x0a'); c.lea_rdx_data(L['varstore_info']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb6\x41\x0c'); c.lea_rdx_data(L['question_flags']); c.emit(b'\x88\x02')
 c.emit(b'\x41\x0f\xb6\x41\x0d'); c.lea_rdx_data(L['oneof_flags']); c.emit(b'\x88\x02')
 c.emit(b'\x41\x0f\xb7\x41\x02')
 c.emit(b'\x66\x85\xc0'); c.rel32(b'\x0f\x84','ifr_next')
 c.lea_rdx_data(L['token']); c.emit(b'\x66\x89\x02')
 # Preserve the next IFR opcode so an unresolved Prompt StringId does not
 # terminate discovery; real firmware can contain sparse language strings.
 c.emit(b'\x4c\x89\xc8\x48\x01\xc8')
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x44\x89\xd0\x29\xc8')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x89\x02')
 # Start every Prompt candidate at the first language package; failures can
 # walk sibling EFI_HII_PACKAGE_STRINGS records in the same package list.
 c.lea_rdx_data(L['strings_first']); c.emit(b'\x48\x8b\x02')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x89\x02')
 serial('ifr')

 c.label('resolve_string_package')
 # Resolve the IFR StringId directly inside the selected Strings package.
 # EFI_HII_SIBT_STRING_SCSU=0x10 / STRINGS_SCSU=0x12 and
 # EFI_HII_SIBT_STRING_UCS2=0x14 / STRINGS_UCS2=0x16 are decoded;
 # FONT variants, DUPLICATE, SKIP1/SKIP2 and EXT1/EXT2/EXT4 are traversed.
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x48\x85\xf6'); c.rel32(b'\x0f\x84','fail_string')
 c.emit(b'\x8b\x06\x25\xff\xff\xff\x00\x89\xc3')
 c.emit(b'\x83\xfb\x2f'); c.rel32(b'\x0f\x82','fail_string')
 c.emit(b'\x8b\x4e\x04\x83\xf9\x2f'); c.rel32(b'\x0f\x82','fail_language')
 c.emit(b'\x39\xd9'); c.rel32(b'\x0f\x87','fail_language')
 c.emit(b'\x80\x7e\x2e\x00'); c.rel32(b'\x0f\x84','fail_language')
 serial('language')
 c.emit(b'\x8b\x46\x08\x39\xc8'); c.rel32(b'\x0f\x82','fail_string')
 c.emit(b'\x39\xd8'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x48\x8d\x3c\x1e\x48\x01\xc6')
 c.emit(b'\x41\xbc\x01\x00\x00\x00')
 c.lea_rdx_data(L['token']); c.emit(b'\x44\x0f\xb7\x2a')

 c.label('string_block_loop')
 c.emit(b'\x48\x39\xfe'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x0f\xb6\x06\x84\xc0'); c.rel32(b'\x0f\x84','fail_string')
 for typ,label in ((0x10,'str_scsu1'),(0x11,'str_scsu1_font'),(0x12,'str_scsun'),(0x13,'str_scsun_font'),
                   (0x14,'str_ucs1'),(0x15,'str_ucs1_font'),(0x16,'str_ucsn'),(0x17,'str_ucsn_font'),
                   (0x20,'str_duplicate'),(0x21,'str_skip2'),(0x22,'str_skip1'),
                   (0x30,'str_ext1'),(0x31,'str_ext2'),(0x32,'str_ext4')):
  c.emit(b'\x3c'+bytes((typ,))); c.rel32(b'\x0f\x84',label)
 c.rel32(b'\xe9','fail_string')

 c.label('str_scsu1')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_scsu1_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x01'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('str_scsu1_found'); c.emit(b'\x48\x83\xc6\x01'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('str_scsu1_font')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_scsu1_font_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x02'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('str_scsu1_font_found'); c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('scan_scsu_single')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x80\x3a\x00'); c.rel32(b'\x0f\x84','scan_scsu_single_done')
 c.emit(b'\x48\xff\xc2'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('scan_scsu_single_done'); c.emit(b'\x48\x8d\x72\x01'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_scsun'); c.emit(b'\x44\x0f\xb7\x76\x01\x48\x8d\x56\x03'); c.rel32(b'\xe9','multi_scsu_loop')
 c.label('str_scsun_font'); c.emit(b'\x44\x0f\xb7\x76\x02\x48\x8d\x56\x04')
 c.label('multi_scsu_loop')
 c.emit(b'\x45\x85\xf6'); c.rel32(b'\x0f\x84','multi_scsu_done')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','multi_scsu_found')
 c.label('multi_scsu_scan')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x80\x3a\x00'); c.rel32(b'\x0f\x84','multi_scsu_next')
 c.emit(b'\x48\xff\xc2'); c.rel32(b'\xe9','multi_scsu_scan')
 c.label('multi_scsu_next'); c.emit(b'\x48\xff\xc2\x41\xff\xc4\x41\xff\xce'); c.rel32(b'\xe9','multi_scsu_loop')
 c.label('multi_scsu_found'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('multi_scsu_done'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_ucs1')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_ucs1_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x01'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('str_ucs1_found'); c.emit(b'\x48\x83\xc6\x01'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('str_ucs1_font')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_ucs1_font_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x02'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('str_ucs1_font_found'); c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('scan_ucs_single')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x66\x83\x3a\x00'); c.rel32(b'\x0f\x84','scan_ucs_single_done')
 c.emit(b'\x48\x83\xc2\x02'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('scan_ucs_single_done'); c.emit(b'\x48\x8d\x72\x02'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_ucsn'); c.emit(b'\x44\x0f\xb7\x76\x01\x48\x8d\x56\x03'); c.rel32(b'\xe9','multi_ucs_loop')
 c.label('str_ucsn_font'); c.emit(b'\x44\x0f\xb7\x76\x02\x48\x8d\x56\x04')
 c.label('multi_ucs_loop')
 c.emit(b'\x45\x85\xf6'); c.rel32(b'\x0f\x84','multi_ucs_done')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','multi_ucs_found')
 c.label('multi_ucs_scan')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x66\x83\x3a\x00'); c.rel32(b'\x0f\x84','multi_ucs_next')
 c.emit(b'\x48\x83\xc2\x02'); c.rel32(b'\xe9','multi_ucs_scan')
 c.label('multi_ucs_next'); c.emit(b'\x48\x83\xc2\x02\x41\xff\xc4\x41\xff\xce'); c.rel32(b'\xe9','multi_ucs_loop')
 c.label('multi_ucs_found'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('multi_ucs_done'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_duplicate')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x85','str_duplicate_skip')
 c.emit(b'\x44\x0f\xb7\x6e\x01\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_string')
 c.emit(b'\x41\xbc\x01\x00\x00\x00')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32\x8b\x46\x08\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_duplicate_skip'); c.emit(b'\x41\xff\xc4\x48\x83\xc6\x03'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_skip1'); c.emit(b'\x0f\xb6\x46\x01\x41\x01\xc4\x48\x83\xc6\x02'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_skip2'); c.emit(b'\x0f\xb7\x46\x01\x41\x01\xc4\x48\x83\xc6\x03'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext1'); c.emit(b'\x0f\xb6\x46\x02\x83\xf8\x03'); c.rel32(b'\x0f\x82','fail_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext2'); c.emit(b'\x0f\xb7\x46\x02\x83\xf8\x04'); c.rel32(b'\x0f\x82','fail_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext4'); c.emit(b'\x8b\x46\x02\x83\xf8\x06'); c.rel32(b'\x0f\x82','fail_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')

 c.label('direct_scsu_found')
 c.emit(b'\x80\x3e\x00'); c.rel32(b'\x0f\x84','fail_string')
 c.emit(b'\x56'); c.rel32(b'\xe8','emit_question_meta'); c.emit(b'\x5e')
 serial('prefix'); c.rel32(b'\xe8','serial_scsu_ascii')
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_string')
 serial('suffix'); c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')
 c.label('direct_ucs_found')
 c.emit(b'\x66\x83\x3e\x00'); c.rel32(b'\x0f\x84','fail_string')
 c.emit(b'\x56'); c.rel32(b'\xe8','emit_question_meta'); c.emit(b'\x5e')
 serial('prefix'); c.rel32(b'\xe8','serial_utf16')
 serial('suffix'); c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')

 c.label('fail_protocol'); serial('no_protocol'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_size'); serial('list_size_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch'); serial('list_fetch_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch_invalid'); serial('list_fetch_invalid'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch_not_found'); serial('list_fetch_not_found'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch_bts_twice'); serial('list_fetch_bts_twice'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_static_small'); serial('list_static_small'); c.rel32(b'\xe9','return_fail')
 c.label('fail_no_handle'); serial('no_handle'); c.rel32(b'\xe9','return_fail')
 c.label('fail_static_empty'); serial('static_empty'); c.rel32(b'\xe9','return_fail')
 c.label('fail_alloc'); serial('alloc'); c.rel32(b'\xe9','return_fail')
 c.label('fail_export'); serial('export'); c.rel32(b'\xe9','return_fail')
 c.label('fail_ifr'); serial('ifr_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_language'); serial('lang_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_string')
 # Try every sibling Strings package (typically alternate languages) before
 # abandoning this Prompt StringId.
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x8b\x06\x25\xff\xff\xff\x00')
 c.emit(b'\x83\xf8\x04'); c.rel32(b'\x0f\x82','prompt_next')
 c.emit(b'\x48\x01\xc6')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x8b\x3a')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x8b\x02\x48\x01\xc7')
 c.label('next_strings_scan')
 c.emit(b'\x48\x8d\x46\x04\x48\x39\xf8'); c.rel32(b'\x0f\x87','prompt_next')
 c.emit(b'\x8b\x06\x89\xc1\x81\xe1\xff\xff\xff\x00')
 c.emit(b'\x89\xc2\xc1\xea\x18')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x82','prompt_next')
 c.emit(b'\x48\x8d\x04\x0e\x48\x39\xf8'); c.rel32(b'\x0f\x87','prompt_next')
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x84','use_next_strings')
 c.emit(b'\x81\xfa\xdf\x00\x00\x00'); c.rel32(b'\x0f\x84','prompt_next')
 c.emit(b'\x48\x89\xc6'); c.rel32(b'\xe9','next_strings_scan')
 c.label('use_next_strings')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x89\x32')
 serial('strings_retry')
 c.rel32(b'\xe9','resolve_string_package')

 c.label('prompt_next')
 # No language package resolved this token: continue with the next real IFR
 # prompt, preserving the firmware statement order.
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x4c\x8b\x0a')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x44\x8b\x12')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x83','ifr_loop')

 c.label('prompt_package_done')
 # This Forms package-list did not yield a resolvable prompt. Continue through
 # the remaining exported HII package-lists rather than producing a false FAIL.
 c.rel32(b'\xe9','direct_list_next')

 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return')
 c.emit(b'\x48\x83\xc4\x68\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5f\x5e\x5d\x5b\xc3')

 # Input rsi=UTF-16 string. Emit low ASCII byte, '?' for non-ASCII codepoints.
 c.label('serial_utf16')
 c.label('utf16_loop')
 c.emit(b'\x66\x8b\x06\x66\x85\xc0'); c.rel32(b'\x0f\x84','utf16_done')
 c.emit(b'\x66\x3d\x7f\x00'); c.rel32(b'\x0f\x87','utf16_question')
 c.emit(b'\x3c\x20'); c.rel32(b'\x0f\x83','utf16_emit')
 c.label('utf16_question'); c.emit(b'\xb0\x3f')
 c.label('utf16_emit')
 c.emit(b'\x88\xc3\x66\xba\xfd\x03')
 c.label('utf16_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'utf16_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\x48\x83\xc6\x02')
 c.rel32(b'\xe9','utf16_loop')
 c.label('utf16_done'); c.emit(b'\xc3')

 c.label('serial_scsu_ascii')
 c.label('scsu_ascii_loop')
 c.emit(b'\x8a\x06\x84\xc0'); c.rel32(b'\x0f\x84','scsu_ascii_ok')
 c.emit(b'\x3c\x20'); c.rel32(b'\x0f\x82','scsu_ascii_bad')
 c.emit(b'\x3c\x7e'); c.rel32(b'\x0f\x87','scsu_ascii_bad')
 c.emit(b'\x88\xc3\x66\xba\xfd\x03')
 c.label('scsu_ascii_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'scsu_ascii_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\x48\xff\xc6'); c.rel32(b'\xe9','scsu_ascii_loop')
 c.label('scsu_ascii_ok'); c.emit(b'\x31\xc0\xc3')
 c.label('scsu_ascii_bad'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('emit_question_meta')
 serial('meta_qid'); c.lea_rsi_data(L['question_id']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('qid_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','qid_hex_loop')
 serial('meta_varstore'); c.lea_rsi_data(L['varstore_id']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('varstore_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','varstore_hex_loop')
 serial('meta_varinfo'); c.lea_rsi_data(L['varstore_info']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('varinfo_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','varinfo_hex_loop')
 serial('meta_qflags'); c.lea_rax_data(L['question_flags']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('meta_oneof'); c.lea_rax_data(L['oneof_flags']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('meta_done'); c.emit(b'\xc3')

 c.label('hex8_emit')
 c.emit(b'\x41\x88\xc3\xc0\xe8\x04'); c.rel32(b'\xe8','hex_nibble_emit')
 c.emit(b'\x44\x88\xd8\x24\x0f'); c.rel32(b'\xe8','hex_nibble_emit'); c.emit(b'\xc3')
 c.label('hex_nibble_emit')
 c.emit(b'\x3c\x09'); c.rel32(b'\x0f\x86','hex_digit')
 c.emit(b'\x04\x37'); c.rel32(b'\xe9','hex_char_ready')
 c.label('hex_digit'); c.emit(b'\x04\x30')
 c.label('hex_char_ready'); c.emit(b'\x88\xc3\x66\xba\xfd\x03')
 c.label('hex_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'hex_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\xc3')

 c.label('serial_emit')
 c.emit(b'\x49\x89\xd0\x66\xba\xfd\x03')
 c.label('serial_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'serial_wait')
 c.emit(b'\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9')
 c.rel8(0x75,'serial_wait'); c.emit(b'\xc3')

 c.patch(); code=bytes(c.data)
 if len(code)>0x3800: raise SystemExit(f'HII prompt code too large: {len(code)}')

 text_raw=0x200; text_raw_size=(len(code)+0x1ff)&~0x1ff
 data_raw=text_raw+text_raw_size; data_raw_size=(len(data)+0x1ff)&~0x1ff
 reloc_rva=(DATA_RVA+len(data)+0xfff)&~0xfff; reloc_raw=data_raw+data_raw_size
 image=bytearray(reloc_raw+0x200); image_size=reloc_rva+0x1000
 put(image,0,'<H',0x5a4d); put(image,0x3c,'<I',0x80)
 pe=0x80; image[pe:pe+4]=b'PE\0\0'; coff=pe+4
 put(image,coff,'<HHIIIHH',0x8664,3,0,0,0,0xf0,0x22); opt=coff+20
 put(image,opt,'<H',0x20b); put(image,opt+4,'<I',text_raw_size)
 put(image,opt+8,'<I',data_raw_size+0x200); put(image,opt+0x10,'<I',TEXT_RVA)
 put(image,opt+0x14,'<I',TEXT_RVA); put(image,opt+0x18,'<Q',0x400000)
 put(image,opt+0x20,'<I',0x1000); put(image,opt+0x24,'<I',0x200)
 put(image,opt+0x38,'<I',image_size); put(image,opt+0x3c,'<I',0x200)
 put(image,opt+0x44,'<H',10); put(image,opt+0x48,'<Q',0x100000)
 put(image,opt+0x50,'<Q',0x1000); put(image,opt+0x58,'<Q',0x100000)
 put(image,opt+0x60,'<Q',0x1000); put(image,opt+0x6c,'<I',16)
 put(image,opt+0x70+5*8,'<II',reloc_rva,8)
 sec=opt+0xf0; image[sec:sec+8]=b'.text\0\0\0'
 put(image,sec+8,'<I',len(code)); put(image,sec+0xc,'<I',TEXT_RVA)
 put(image,sec+0x10,'<I',text_raw_size); put(image,sec+0x14,'<I',text_raw)
 put(image,sec+0x24,'<I',0x60000020)
 ds=sec+40; image[ds:ds+8]=b'.data\0\0\0'
 put(image,ds+8,'<I',len(data)); put(image,ds+0xc,'<I',DATA_RVA)
 put(image,ds+0x10,'<I',data_raw_size); put(image,ds+0x14,'<I',data_raw)
 put(image,ds+0x24,'<I',0xc0000040)
 rs=sec+80; image[rs:rs+8]=b'.reloc\0\0'
 put(image,rs+8,'<I',8); put(image,rs+0xc,'<I',reloc_rva)
 put(image,rs+0x10,'<I',0x200); put(image,rs+0x14,'<I',reloc_raw)
 put(image,rs+0x24,'<I',0x42000040)
 image[text_raw:text_raw+len(code)]=code; image[data_raw:data_raw+len(data)]=data
 put(image,reloc_raw,'<II',TEXT_RVA,8)
 return bytes(image)

def validate(image):
 assert image[:2]==b'MZ'
 for token in (
  b'HII_DATABASE_PROTOCOL=PASS',
  b'HII_EXPORT_ALL_PACKAGE_LISTS=PASS',
  b'HII_FORMS_PACKAGE=PASS',
  b'HII_STRINGS_PACKAGE=PASS',
  b'IFR_QUESTION_PROMPT_STRING_ID=PASS',
  b'QUESTION_TEXT=',
  b'HII_QUESTION_STRING=PASS',
  b'QUESTION_METADATA=PASS',
  b'NONZERO_VARSTORE_ID=PASS',
  b'QUESTION_ID_LE_HEX=',
  b'VARSTORE_ID_LE_HEX=',
  b'VARSTORE_INFO_LE_HEX=',
  b'QUESTION_FLAGS_HEX=',
  b'ONEOF_FLAGS_HEX=',
 ):
  assert token in image,token

def main():
 if len(sys.argv)!=2: raise SystemExit('usage: build_uefi_hii_stored_question.py OUTPUT_EFI')
 image=build(); validate(image)
 p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HII_STORED_QUESTION_BUILD=PASS')
 print('bytes='+str(len(image)))
 print('sha256='+hashlib.sha256(image).hexdigest())

if __name__=='__main__':
 main()
