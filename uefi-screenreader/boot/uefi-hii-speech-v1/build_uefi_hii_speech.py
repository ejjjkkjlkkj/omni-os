#!/usr/bin/env python3
from __future__ import annotations
import hashlib, importlib.util, math, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x8000
RATE=48000
CHANNELS=2
BITS=16
TONE_HZ=660.0
TONE_MS=320
DMA_PAGES=128
PCM_OFF=0x1000

MARKS={
 'start': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATE=START\r\nSYNTH=ALLOPHONE_BDL_RUNTIME_V1\r\nMODE=HII_TITLE_TO_HDA_SPEECH\r\nTRANSPORT=HDA_NATIVE_DMA\r\nEND\r\n',
 'wait': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nEVENT=HII_TITLE_READY\r\nSUPPORTED=a-z\r\nMAX_SPOKEN_GRAPHEMES=8\r\nEND\r\n',
 'char': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nEVENT=HII_GRAPHEME_ACCEPTED\r\nEND\r\n',
 'text_ready': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nEVENT=HII_TEXT_COMMIT\r\nTEXT_BUFFER=PASS\r\nBDL_RUNTIME_TEXT_SCHEDULE=PASS\r\nEND\r\n',
 'database': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_DATABASE_PROTOCOL=PASS\r\nEND\r\n',
 'string_protocol': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_STRING_PROTOCOL=PASS\r\nEND\r\n',
 'direct_export': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_EXPORT_ALL_PACKAGE_LISTS=PASS\r\nEND\r\n',
 'forms_package_seen': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_FORMS_PACKAGE=PASS\r\nEND\r\n',
 'strings_package': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_STRINGS_PACKAGE=PASS\r\nEND\r\n',
 'forms_handle': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_FORMS_HANDLE=PASS\r\nEND\r\n',
 'ifr': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nIFR_TITLE_STRING_ID=PASS\r\nEND\r\n',
 'language': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_LANGUAGE=PASS\r\nEND\r\n',
 'hii': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHII_TITLE_SOURCE=PASS\r\nEND\r\n',
 'hii_protocol_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_PROTOCOL_NOT_FOUND\r\nEND\r\n',
 'hii_list_size_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_SIZE_QUERY_FAILED\r\nEND\r\n',
 'hii_list_fetch_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_FAILED\r\nEND\r\n',
 'hii_list_fetch_invalid': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_EFI_INVALID_PARAMETER\r\nEND\r\n',
 'hii_list_fetch_not_found': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_EFI_NOT_FOUND\r\nEND\r\n',
 'hii_list_static_small': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_STATIC_BUFFER_TOO_SMALL\r\nEND\r\n',
 'hii_handle_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_FORMS_HANDLE_NOT_FOUND\r\nEND\r\n',
 'hii_alloc_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_POOL_ALLOC_FAILED\r\nEND\r\n',
 'hii_export_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_EXPORT_FAILED\r\nEND\r\n',
 'hii_ifr_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=IFR_TITLE_TOKEN_NOT_FOUND\r\nEND\r\n',
 'hii_language_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LANGUAGE_NOT_FOUND\r\nEND\r\n',
 'hii_string_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_TITLE_STRING_NOT_RESOLVED\r\nEND\r\n',
 'controller': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nHDA_CONTROLLER_CODEC=PASS\r\nEND\r\n',
 'topology': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nAFG_RUNTIME_DISCOVERY=PASS\r\nAUTO_DAC_WIDGET=PASS\r\nAUTO_OUTPUT_PIN_WIDGET=PASS\r\nAUTO_PIN_TO_DAC_DIRECT_ROUTE=PASS\r\nNO_FIXED_WIDGET_NIDS=PASS\r\nEND\r\n',
 'policy': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nPIN_CONFIG_DEFAULT=PASS\r\nPIN_CAPABILITIES_QUERY=PASS\r\nEAPD_IF_SUPPORTED=PASS\r\nDAC_OUTPUT_AMP_VERIFY=PASS\r\nEND\r\n',
 'dma': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nUNIT_BANK_COPY=PASS\r\nBDL_RUNTIME_TEXT_SCHEDULE=PASS\r\nEND\r\n',
 'codec': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nCODEC_DAC_STREAM=PASS\r\nCODEC_PIN_OUTPUT=PASS\r\nEND\r\n',
 'stream': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nOUTPUT_STREAM_DESCRIPTOR=PASS\r\nFORMAT_48K_S16_STEREO=PASS\r\nBDL_ENTRIES=RUNTIME\r\nEND\r\n',
 'progress': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nLPIB_PROGRESS=PASS\r\nHII_TITLE_SPEECH_HDA=PASS\r\nEND\r\n',
 'done': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=PASS\r\nEND\r\n',
 'no_hda': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n',
 'bad_hda': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CONTROLLER_OR_CODEC_FAILED\r\nEND\r\n',
 'alloc': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=DMA_ALLOC_FAILED\r\nEND\r\n',
 'verb': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=CODEC_VERB_FAILED\r\nEND\r\n',
 'stream_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=STREAM_DMA_NO_PROGRESS\r\nEND\r\n',
 'key_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=TEXT_INPUT_INVALID_OR_TOO_LONG\r\nEND\r\n',
 'topology_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=AUTO_OUTPUT_ROUTE_DISCOVERY_FAILED\r\nEND\r\n',
 'policy_fail': b'QEVARYNOX-UEFI-HII-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=OUTPUT_POLICY_FAILED\r\nEND\r\n',
}

ROOT=Path(__file__).resolve().parents[2]
SPEECH_BUILDER=ROOT/'boot'/'uefi-native-speech-v1'/'build_uefi_native_speech.py'
LETTER_UNITS={
 'a':('a',),
 'b':('b','e'),
 'c':('s','e'),
 'd':('d','e'),
 'e':('e',),
 'f':('e','f'),
 'g':('sh','e'),
 'h':('a','sh'),
 'i':('i',),
 'j':('sh','i'),
 'k':('k','a'),
 'l':('e','l'),
 'm':('e','m'),
 'n':('e','n'),
 'o':('o',),
 'p':('p','e'),
 'q':('k','u'),
 'r':('e','r'),
 's':('e','s'),
 't':('t','e'),
 'u':('u',),
 'v':('v','e'),
 'w':('d','u','b','l','e','v','e'),
 'x':('i','k','s'),
 'y':('i','g','r','e','k'),
 'z':('z','e','d'),
}
TEXT_UNITS=tuple(sorted({name for sequence in LETTER_UNITS.values() for name in sequence}))
UNIT_LAYOUT: dict[str, tuple[int,int]] = {}

def load_module(name: str, path: Path):
 spec=importlib.util.spec_from_file_location(name,path)
 if spec is None or spec.loader is None:
  raise SystemExit(f'cannot load {path}')
 module=importlib.util.module_from_spec(spec)
 spec.loader.exec_module(module)
 return module

speech=load_module('qevarynx_native_speech_runtime_source',SPEECH_BUILDER)
class Code:
 def __init__(self): self.data=bytearray(); self.labels={}; self.fix=[]
 def pos(self): return len(self.data)
 def emit(self,b): self.data += bytes(b)
 def label(self,n): self.labels[n]=self.pos()
 def rel32(self,op,label):
  self.emit(op); off=self.pos(); self.emit(b'\0'*4); self.fix.append((off,self.pos(),label,4))
 def rel8(self,op,label):
  self.emit(bytes((op,0))); self.fix.append((self.pos()-1,self.pos(),label,1))
 def data_disp(self,opcode,off):
  self.emit(opcode); after=TEXT_RVA+self.pos()+4; self.emit(struct.pack('<i',DATA_RVA+off-after))
 def lea_rax_data(self,off): self.data_disp(b'\x48\x8d\x05',off)
 def lea_rcx_data(self,off): self.data_disp(b'\x48\x8d\x0d',off)
 def lea_r8_data(self,off): self.data_disp(b'\x4c\x8d\x05',off)
 def lea_rdx_data(self,off): self.data_disp(b'\x48\x8d\x15',off)
 def lea_rsi_data(self,off): self.data_disp(b'\x48\x8d\x35',off)
 def lea_r9_data(self,off): self.data_disp(b'\x4c\x8d\x0d',off)
 def mov_r13_data(self,off): self.data_disp(b'\x4c\x8b\x2d',off)
 def patch(self):
  for off,after,label,width in self.fix:
   disp=self.labels[label]-after
   if width==4: struct.pack_into('<i',self.data,off,disp)
   else:
    if not -128<=disp<=127: raise SystemExit(f'short overflow {label}: {disp}')
    self.data[off]=disp&0xff

def put(b,o,f,*v): struct.pack_into(f,b,o,*v)

def convert_unit(raw: bytes) -> bytes:
 out=bytearray()
 for sample in raw:
  signed=max(-32768,min(32767,(sample-128)*180))
  frame=struct.pack('<hh',signed,signed)
  out += frame*6
 return bytes(out)

def make_pcm():
 units=speech.make_units()
 converted={name:convert_unit(units[name]) for name in TEXT_UNITS}
 bank=bytearray()
 UNIT_LAYOUT.clear()
 for name in TEXT_UNITS:
  off=len(bank); raw=converted[name]
  UNIT_LAYOUT[name]=(off,len(raw)); bank += raw
 capacity=DMA_PAGES*4096-PCM_OFF
 if len(bank)>capacity:
  raise SystemExit(f'unit bank exceeds DMA allocation: {len(bank)} > {capacity}')
 return bytes(bank)
def build():
 pcm=make_pcm()
 data=bytearray(0x101100)
 L={'maxaddr':0,'keybuf':8,'dac_nid':16,'pin_nid':20,'textbuf':32,'text_count':56,'db_guid':64,'str_guid':80,'dbptr':96,'strptr':104,'handles_size':112,'handles_ptr':120,'pkg_size':128,'pkg_ptr':136,'langs_size':144,'langs_ptr':152,'string_size':160,'string_ptr':168,'token':176,'temp_handle':184,'handle_cursor':192,'handles_remaining':200,'forms_ptr':208,'strings_ptr':216,'list_len':224,'handles_static':0x100,'pkg_static':0x1100}
 struct.pack_into('<Q',data,0,0xffffffff)
 struct.pack_into('<IHH8B',data,L['db_guid'],0xef9fc172,0xa1b2,0x4693,0xb3,0x27,0x6d,0x32,0xfc,0x41,0x60,0x42)
 struct.pack_into('<IHH8B',data,L['str_guid'],0x0fd96974,0x23aa,0x4cdc,0xb9,0xcb,0x98,0xd1,0x77,0x50,0x32,0x2a)
 for n,m in MARKS.items(): L[n]=len(data); data+=m
 L['pcm']=len(data); data+=pcm

 c=Code()
 # Preserve all nonvolatile registers touched by the raw EFI entry point.
 c.emit(b'\x53\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57')
 c.emit(b'\x48\x8b\x6a\x30\x4c\x8b\x7a\x60')  # rbp = ConIn, r15 = BootServices
 c.emit(b'\x48\x83\xec\x68\xfc')
 for p,v in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
  c.emit(b'\x66\xba'+struct.pack('<H',p)+b'\xb0'+bytes((v,))+b'\xee')
 def serial(n):
  c.lea_rdx_data(L[n]); c.emit(b'\xb9'+struct.pack('<I',len(MARKS[n]))); c.rel32(b'\xe8','serial_emit')
 def verb_const(value):
  # OR codec address (r12d) into command constant and execute Immediate Command.
  c.emit(b'\x44\x89\xe0\xc1\xe0\x1c')
  c.emit(b'\x0d'+struct.pack('<I',value))
  c.rel32(b'\xe8','immediate')
  c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb')
 def verb_data(nid_off,value):
  c.emit(b'\x44\x89\xe0\xc1\xe0\x1c')
  c.lea_rdx_data(nid_off)
  c.emit(b'\x8b\x0a\xc1\xe1\x14\x09\xc8')
  c.emit(b'\x0d'+struct.pack('<I',value))
  c.rel32(b'\xe8','immediate')
  c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb')

 def zero_qword(off):
  c.lea_rdx_data(off); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 def alloc(size_reg_is_rbx,ptr_off):
  c.emit(b'\xb9\x04\x00\x00\x00')
  if size_reg_is_rbx: c.emit(b'\x48\x89\xda')
  c.lea_r8_data(ptr_off)
  c.emit(b'\x41\xff\x57\x40')
  c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_hii_alloc')

 serial('start')

 # Locate HII Database protocol -> r12.
 c.lea_rcx_data(L['db_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['dbptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_hii_protocol')
 c.lea_rdx_data(L['dbptr']); c.emit(b'\x4c\x8b\x22')
 c.emit(b'\x4d\x85\xe4'); c.rel32(b'\x0f\x84','fail_hii_protocol')
 serial('database')

 # Locate HII String protocol -> r13.
 c.lea_rcx_data(L['str_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['strptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_hii_protocol')
 c.lea_rdx_data(L['strptr']); c.emit(b'\x4c\x8b\x2a')
 c.emit(b'\x4d\x85\xed'); c.rel32(b'\x0f\x84','fail_hii_protocol')
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
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_hii_export')
 c.lea_rdx_data(L['pkg_size']); c.emit(b'\x48\x83\x3a\x18')
 c.rel32(b'\x0f\x82','fail_hii_export')
 serial('direct_export')

 # Parse concatenated EFI_HII_PACKAGE_LIST_HEADER records.  Select one package
 # list containing both Forms and Strings so the IFR StringId and text share
 # the same package-list namespace.
 c.lea_rdx_data(L['pkg_ptr']); c.emit(b'\x48\x8b\x32')
 c.lea_rdx_data(L['pkg_size']); c.emit(b'\x48\x8b\x1a')
 c.label('direct_list_loop')
 c.emit(b'\x48\x83\xfb\x14'); c.rel32(b'\x0f\x82','fail_hii_handle')
 c.emit(b'\x8b\x46\x10\x83\xf8\x18'); c.rel32(b'\x0f\x82','fail_hii_ifr')
 c.emit(b'\x48\x39\xd8'); c.rel32(b'\x0f\x87','fail_hii_ifr')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x89\x02')
 zero_qword(L['forms_ptr']); zero_qword(L['strings_ptr'])
 c.emit(b'\x48\x8d\x7e\x14\x89\xc1\x83\xe9\x14')
 c.label('direct_pkg_loop')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x82','fail_hii_ifr')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00')
 c.emit(b'\x89\xc5\xc1\xed\x18')
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x82','fail_hii_ifr')
 c.emit(b'\x39\xca'); c.rel32(b'\x0f\x87','fail_hii_ifr')
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
 serial('forms_package_seen'); serial('strings_package')
 c.lea_rdx_data(L['forms_ptr']); c.emit(b'\x48\x8b\x3a')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00')
 c.rel32(b'\xe9','forms_pkg')

 c.label('direct_list_next')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x8b\x02')
 c.emit(b'\x48\x01\xc6\x48\x29\xc3')
 c.emit(b'\x48\x85\xdb'); c.rel32(b'\x0f\x85','direct_list_loop')
 c.rel32(b'\xe9','fail_hii_handle')

 c.label('forms_pkg')
 # r9=first IFR opcode, r10d=bytes available in verified Forms package.
 c.emit(b'\x4c\x8d\x4f\x04\x41\x89\xd2\x41\x83\xea\x04')
 c.label('ifr_loop')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','fail_hii_ifr')
 c.emit(b'\x41\x0f\xb6\x01')       # eax=OpCode
 c.emit(b'\x41\x0f\xb6\x49\x01\x83\xe1\x7f') # ecx=Length
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','fail_hii_ifr')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','fail_hii_ifr')
 c.emit(b'\x3c\x0e'); c.rel32(b'\x0f\x84','ifr_formset')
 c.emit(b'\x3c\x01'); c.rel32(b'\x0f\x84','ifr_form')
 c.label('ifr_next')
 c.emit(b'\x49\x01\xc9\x41\x29\xca')
 c.rel32(b'\xe9','ifr_loop')

 c.label('ifr_formset')
 c.emit(b'\x83\xf9\x17'); c.rel32(b'\x0f\x82','ifr_next')
 c.emit(b'\x41\x0f\xb7\x41\x12'); c.rel32(b'\xe9','token_candidate')
 c.label('ifr_form')
 c.emit(b'\x83\xf9\x06'); c.rel32(b'\x0f\x82','ifr_next')
 c.emit(b'\x41\x0f\xb7\x41\x04')
 c.label('token_candidate')
 c.emit(b'\x66\x85\xc0'); c.rel32(b'\x0f\x84','ifr_next')
 c.lea_rdx_data(L['token']); c.emit(b'\x66\x89\x02')
 serial('ifr')

 # Resolve the IFR StringId directly inside the selected Strings package.
 # EFI_HII_SIBT_STRING_SCSU=0x10 / STRINGS_SCSU=0x12 and
 # EFI_HII_SIBT_STRING_UCS2=0x14 / STRINGS_UCS2=0x16 are decoded;
 # FONT variants, DUPLICATE, SKIP1/SKIP2 and EXT1/EXT2/EXT4 are traversed.
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x48\x85\xf6'); c.rel32(b'\x0f\x84','fail_hii_string')
 c.emit(b'\x8b\x06\x25\xff\xff\xff\x00\x89\xc3')
 c.emit(b'\x83\xfb\x2f'); c.rel32(b'\x0f\x82','fail_hii_string')
 c.emit(b'\x8b\x4e\x04\x83\xf9\x2f'); c.rel32(b'\x0f\x82','fail_hii_language')
 c.emit(b'\x39\xd9'); c.rel32(b'\x0f\x87','fail_hii_language')
 c.emit(b'\x80\x7e\x2e\x00'); c.rel32(b'\x0f\x84','fail_hii_language')
 serial('language')
 c.emit(b'\x8b\x46\x08\x39\xc8'); c.rel32(b'\x0f\x82','fail_hii_string')
 c.emit(b'\x39\xd8'); c.rel32(b'\x0f\x83','fail_hii_string')
 c.emit(b'\x48\x8d\x3c\x1e\x48\x01\xc6')
 c.emit(b'\x41\xbc\x01\x00\x00\x00')
 c.lea_rdx_data(L['token']); c.emit(b'\x44\x0f\xb7\x2a')

 c.label('string_block_loop')
 c.emit(b'\x48\x39\xfe'); c.rel32(b'\x0f\x83','fail_hii_string')
 c.emit(b'\x0f\xb6\x06\x84\xc0'); c.rel32(b'\x0f\x84','fail_hii_string')
 for typ,label in ((0x10,'str_scsu1'),(0x11,'str_scsu1_font'),(0x12,'str_scsun'),(0x13,'str_scsun_font'),
                   (0x14,'str_ucs1'),(0x15,'str_ucs1_font'),(0x16,'str_ucsn'),(0x17,'str_ucsn_font'),
                   (0x20,'str_duplicate'),(0x21,'str_skip2'),(0x22,'str_skip1'),
                   (0x30,'str_ext1'),(0x31,'str_ext2'),(0x32,'str_ext4')):
  c.emit(b'\x3c'+bytes((typ,))); c.rel32(b'\x0f\x84',label)
 c.rel32(b'\xe9','fail_hii_string')

 c.label('str_scsu1')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_scsu1_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x01'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('str_scsu1_found'); c.emit(b'\x48\x83\xc6\x01'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('str_scsu1_font')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_scsu1_font_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x02'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('str_scsu1_font_found'); c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('scan_scsu_single')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_hii_string')
 c.emit(b'\x80\x3a\x00'); c.rel32(b'\x0f\x84','scan_scsu_single_done')
 c.emit(b'\x48\xff\xc2'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('scan_scsu_single_done'); c.emit(b'\x48\x8d\x72\x01'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_scsun'); c.emit(b'\x44\x0f\xb7\x76\x01\x48\x8d\x56\x03'); c.rel32(b'\xe9','multi_scsu_loop')
 c.label('str_scsun_font'); c.emit(b'\x44\x0f\xb7\x76\x02\x48\x8d\x56\x04')
 c.label('multi_scsu_loop')
 c.emit(b'\x45\x85\xf6'); c.rel32(b'\x0f\x84','multi_scsu_done')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','multi_scsu_found')
 c.label('multi_scsu_scan')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_hii_string')
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
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_hii_string')
 c.emit(b'\x66\x83\x3a\x00'); c.rel32(b'\x0f\x84','scan_ucs_single_done')
 c.emit(b'\x48\x83\xc2\x02'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('scan_ucs_single_done'); c.emit(b'\x48\x8d\x72\x02'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_ucsn'); c.emit(b'\x44\x0f\xb7\x76\x01\x48\x8d\x56\x03'); c.rel32(b'\xe9','multi_ucs_loop')
 c.label('str_ucsn_font'); c.emit(b'\x44\x0f\xb7\x76\x02\x48\x8d\x56\x04')
 c.label('multi_ucs_loop')
 c.emit(b'\x45\x85\xf6'); c.rel32(b'\x0f\x84','multi_ucs_done')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','multi_ucs_found')
 c.label('multi_ucs_scan')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_hii_string')
 c.emit(b'\x66\x83\x3a\x00'); c.rel32(b'\x0f\x84','multi_ucs_next')
 c.emit(b'\x48\x83\xc2\x02'); c.rel32(b'\xe9','multi_ucs_scan')
 c.label('multi_ucs_next'); c.emit(b'\x48\x83\xc2\x02\x41\xff\xc4\x41\xff\xce'); c.rel32(b'\xe9','multi_ucs_loop')
 c.label('multi_ucs_found'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('multi_ucs_done'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_duplicate')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x85','str_duplicate_skip')
 c.emit(b'\x44\x0f\xb7\x6e\x01\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_hii_string')
 c.emit(b'\x41\xbc\x01\x00\x00\x00')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32\x8b\x46\x08\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_duplicate_skip'); c.emit(b'\x41\xff\xc4\x48\x83\xc6\x03'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_skip1'); c.emit(b'\x0f\xb6\x46\x01\x41\x01\xc4\x48\x83\xc6\x02'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_skip2'); c.emit(b'\x0f\xb7\x46\x01\x41\x01\xc4\x48\x83\xc6\x03'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext1'); c.emit(b'\x0f\xb6\x46\x02\x83\xf8\x03'); c.rel32(b'\x0f\x82','fail_hii_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext2'); c.emit(b'\x0f\xb7\x46\x02\x83\xf8\x04'); c.rel32(b'\x0f\x82','fail_hii_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext4'); c.emit(b'\x8b\x46\x02\x83\xf8\x06'); c.rel32(b'\x0f\x82','fail_hii_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')

 c.label('direct_scsu_found')
 c.emit(b'\x80\x3e\x00'); c.rel32(b'\x0f\x84','fail_hii_string')
 c.emit(b'\x31\xff')
 c.label('capture_scsu_loop')
 c.emit(b'\x8a\x06\x84\xc0'); c.rel32(b'\x0f\x84','capture_done')
 c.emit(b'\x48\xff\xc6\x0c\x20')
 c.emit(b'\x3c\x61'); c.rel32(b'\x0f\x82','capture_scsu_loop')
 c.emit(b'\x3c\x7a'); c.rel32(b'\x0f\x87','capture_scsu_loop')
 c.emit(b'\x83\xff\x08'); c.rel32(b'\x0f\x83','capture_done')
 c.emit(b'\x41\x89\xc3'); serial('char')
 c.lea_rdx_data(L['textbuf']); c.emit(b'\x89\xf8\x48\x8d\x04\x42\x66\x44\x89\x18\xff\xc7')
 c.rel32(b'\xe9','capture_scsu_loop')

 c.label('direct_ucs_found')
 c.emit(b'\x66\x83\x3e\x00'); c.rel32(b'\x0f\x84','fail_hii_string')
 c.emit(b'\x31\xff')
 c.label('capture_ucs_loop')
 c.emit(b'\x0f\xb7\x06\x85\xc0'); c.rel32(b'\x0f\x84','capture_done')
 c.emit(b'\x48\x83\xc6\x02\x83\xc8\x20')
 c.emit(b'\x83\xf8\x61'); c.rel32(b'\x0f\x82','capture_ucs_loop')
 c.emit(b'\x83\xf8\x7a'); c.rel32(b'\x0f\x87','capture_ucs_loop')
 c.emit(b'\x83\xff\x08'); c.rel32(b'\x0f\x83','capture_done')
 c.emit(b'\x41\x89\xc3'); serial('char')
 c.lea_rdx_data(L['textbuf']); c.emit(b'\x89\xf8\x48\x8d\x04\x42\x66\x44\x89\x18\xff\xc7')
 c.rel32(b'\xe9','capture_ucs_loop')

 c.label('capture_done')
 c.emit(b'\x85\xff'); c.rel32(b'\x0f\x84','fail_hii_string')
 c.lea_rdx_data(L['textbuf']); c.emit(b'\x89\xf8\x48\x8d\x04\x42\x66\xc7\x00\x00\x00')
 c.lea_rdx_data(L['text_count']); c.emit(b'\x89\x3a')
 serial('hii')

 # Scan full PCI config mechanism-1 segment for class 04/subclass 03.
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
 # Enable PCI memory space and bus mastering.
 c.emit(b'\x44\x89\xe8\x83\xc8\x04'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x89\xc1\x83\xc9\x06\x44\x89\xe8\x83\xc8\x04'); c.rel32(b'\xe8','pci_write32')
 # BAR0, including 64-bit BARs.
 c.emit(b'\x44\x89\xe8\x83\xc8\x10'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x41\x89\xc6\xa8\x01'); c.rel32(b'\x0f\x85','fail_bad_hda')
 c.emit(b'\x89\xc1\x83\xe1\x06\x83\xf9\x04'); c.rel32(b'\x0f\x85','bar_low')
 c.emit(b'\x44\x89\xe8\x83\xc8\x14'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x48\xc1\xe0\x20\x49\x09\xc6')
 c.label('bar_low')
 c.emit(b'\x49\x81\xe6\xf0\xff\xff\xff\x4d\x85\xf6'); c.rel32(b'\x0f\x84','fail_bad_hda')
 # Move BAR to r14 and sanity check GCAP/version.
 c.emit(b'\x4d\x89\xf6')
 c.emit(b'\x41\x0f\xb7\x06\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x41\x0f\xb6\x46\x03\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 # Controller reset.
 c.emit(b'\x41\x8b\x46\x08\x83\xe0\xfe\x41\x89\x46\x08\xb9\xa0\x86\x01\x00')
 c.label('reset_clear_poll'); c.emit(b'\x41\x8b\x46\x08\xa8\x01'); c.rel32(b'\x0f\x84','reset_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','reset_clear_poll'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('reset_clear_ok')
 c.emit(b'\xb9\x64\x00\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x41\x8b\x46\x08\x83\xc8\x01\x41\x89\x46\x08\xb9\xa0\x86\x01\x00')
 c.label('reset_set_poll'); c.emit(b'\x41\x8b\x46\x08\xa8\x01'); c.rel32(b'\x0f\x85','reset_set_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','reset_set_poll'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('reset_set_ok')
 c.emit(b'\xb9\xe8\x03\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 # Pick first codec address from STATESTS.
 c.emit(b'\x41\x0f\xb7\x46\x0e\x66\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x45\x31\xe4')
 c.label('cad_loop')
 c.emit(b'\xa8\x01'); c.rel32(b'\x0f\x85','cad_found')
 c.emit(b'\x66\xd1\xe8\x41\xff\xc4\x41\x83\xfc\x0f'); c.rel32(b'\x0f\x82','cad_loop')
 c.rel32(b'\xe9','fail_bad_hda')
 c.label('cad_found')
 serial('controller')

 # Discover Audio Function Group and its widget range.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x04\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x41\x89\xc5\x41\x81\xe5\xff\x00\x00\x00\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc6\xc1\xee\x10\x81\xe6\xff\x00\x00\x00\x85\xf6'); c.rel32(b'\x0f\x84','fail_topology')
 c.label('afg_loop')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x05\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x01'); c.rel32(b'\x0f\x84','afg_found')
 c.emit(b'\xff\xc6\x41\xff\xcd'); c.rel32(b'\x0f\x85','afg_loop')
 c.rel32(b'\xe9','fail_topology')
 c.label('afg_found')

 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x04\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x41\x89\xc5\x41\x81\xe5\xff\x00\x00\x00\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc6\xc1\xee\x10\x81\xe6\xff\x00\x00\x00\x85\xf6'); c.rel32(b'\x0f\x84','fail_topology')

 # r10d = DAC NID; r11d = output Pin NID.
 c.emit(b'\x45\x31\xd2\x45\x31\xdb')
 c.label('widget_scan')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x09\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc1\xc1\xe9\x14\x83\xe1\x0f')
 c.emit(b'\x83\xf9\x00'); c.rel32(b'\x0f\x85','maybe_pin')
 c.emit(b'\x45\x85\xd2'); c.rel32(b'\x0f\x85','widget_next')
 c.emit(b'\x41\x89\xf2'); c.rel32(b'\xe9','widget_next')
 c.label('maybe_pin')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x85','widget_next')
 c.emit(b'\x45\x85\xdb'); c.rel32(b'\x0f\x85','widget_next')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x0c\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\xa8\x10'); c.rel32(b'\x0f\x84','widget_next')
 c.emit(b'\x41\x89\xf3')
 c.label('widget_next')
 c.emit(b'\xff\xc6\x41\xff\xcd'); c.rel32(b'\x0f\x85','widget_scan')
 c.emit(b'\x45\x85\xd2'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x45\x85\xdb'); c.rel32(b'\x0f\x84','fail_topology')

 # Direct route: selected pin must connect to selected DAC.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x44\x89\xd9\xc1\xe1\x14\x09\xc8\x0d\x0e\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc1\x80\xe1\x7f'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\xa8\x80'); c.rel32(b'\x0f\x85','route_long')
 c.label('route_short')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x44\x89\xd9\xc1\xe1\x14\x09\xc8\x0d\x00\x02\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x25\x7f\x00\x00\x00\x44\x39\xd0'); c.rel32(b'\x0f\x85','fail_topology')
 c.rel32(b'\xe9','route_ok')
 c.label('route_long')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x44\x89\xd9\xc1\xe1\x14\x09\xc8\x0d\x00\x02\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x25\xff\x7f\x00\x00\x44\x39\xd0'); c.rel32(b'\x0f\x85','fail_topology')
 c.label('route_ok')

 c.lea_rdx_data(L['dac_nid']); c.emit(b'\x44\x89\x12')
 c.lea_rdx_data(L['pin_nid']); c.emit(b'\x44\x89\x1a')
 serial('topology')

 # Apply runtime output policy to the selected pin and DAC.
 verb_data(L['pin_nid'],0x000f1c00)
 c.emit(b'\xc1\xe8\x14\x83\xe0\x0f\x83\xf8\x02'); c.rel32(b'\x0f\x87','fail_policy')
 # Query Pin Capabilities. If EAPD is advertised (bit 16), enable it and
 # read it back before speech. Unsupported pins take the verified skip path.
 verb_data(L['pin_nid'],0x000f000c)
 c.emit(b'\xa9\x00\x00\x01\x00'); c.rel32(b'\x0f\x84','eapd_done')
 verb_data(L['pin_nid'],0x00070c02)
 verb_data(L['pin_nid'],0x000f0c00)
 c.emit(b'\xa8\x02'); c.rel32(b'\x0f\x84','fail_policy')
 c.label('eapd_done')
 verb_data(L['dac_nid'],0x000f0012)
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x84','fail_policy')
 verb_data(L['dac_nid'],0x0003b040)
 verb_data(L['dac_nid'],0x000ba000)
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x40'); c.rel32(b'\x0f\x85','fail_policy')
 verb_data(L['dac_nid'],0x000b8000)
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x40'); c.rel32(b'\x0f\x85','fail_policy')
 serial('policy')

 # Allocate DMA pages below 4 GiB: BDL at base, unit bank at base+0x1000.
 c.emit(b'\xb9\x01\x00\x00\x00\xba\x04\x00\x00\x00')
 c.emit(b'\x41\xb8'+struct.pack('<I',DMA_PAGES))
 c.lea_r9_data(L['maxaddr'])
 c.emit(b'\x49\x8b\x47\x28\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_alloc')
 c.mov_r13_data(L['maxaddr'])
 c.emit(b'\x4d\x85\xed'); c.rel32(b'\x0f\x84','fail_alloc')
 # Copy the unordered first-party allophone bank into DMA memory.
 c.lea_rsi_data(L['pcm'])
 c.emit(b'\x49\x8d\xbd'+struct.pack('<i',PCM_OFF))
 c.emit(b'\xb9'+struct.pack('<I',len(pcm))+b'\xf3\xa4')
 # Native HII title input was decoded into textbuf before HDA setup.
 c.lea_rdx_data(L['text_count']); c.emit(bytes.fromhex('8b3a'))
 c.emit(bytes.fromhex('85ff')); c.rel32(bytes.fromhex('0f84'),'fail_key')
 c.label('text_commit')
 c.emit(bytes.fromhex('85ff')); c.rel32(bytes.fromhex('0f84'),'fail_key')
 c.lea_rdx_data(L['textbuf'])
 c.emit(bytes.fromhex('89f8488d044266c7000000'))
 c.emit(bytes.fromhex('31f6'))      # esi = character index
 c.emit(bytes.fromhex('31db'))      # ebx = BDL descriptor count
 c.emit(bytes.fromhex('4531d2'))    # r10d = total PCM bytes
 c.label('expand_char')
 c.lea_rdx_data(L['textbuf'])
 c.emit(bytes.fromhex('89f00fb70442'))
 for ch in LETTER_UNITS:
  c.emit(bytes.fromhex('663d')+struct.pack('<H',ord(ch)))
  c.rel32(bytes.fromhex('0f84'),'expand_'+ch)
 c.rel32(bytes.fromhex('e9'),'fail_key')

 def emit_unit_descriptor(name):
  unit_off,unit_len=UNIT_LAYOUT[name]
  c.emit(bytes.fromhex('89d848c1e0044c01e8'))
  c.emit(bytes.fromhex('498d95')+struct.pack('<i',PCM_OFF+unit_off))
  c.emit(bytes.fromhex('488910'))
  c.emit(bytes.fromhex('c74008')+struct.pack('<I',unit_len))
  c.emit(bytes.fromhex('c7400c00000000'))
  c.emit(bytes.fromhex('ffc3'))
  c.emit(bytes.fromhex('4181c2')+struct.pack('<I',unit_len))

 def emit_expansion(ch,sequence):
  c.label('expand_'+ch)
  for name in sequence:
   emit_unit_descriptor(name)
  c.rel32(bytes.fromhex('e9'),'expanded_char')

 for ch,sequence in LETTER_UNITS.items():
  emit_expansion(ch,sequence)

 c.label('expanded_char')
 c.emit(bytes.fromhex('ffc6'))
 c.emit(bytes.fromhex('39fe'))
 c.rel32(bytes.fromhex('0f82'),'expand_char')
 c.emit(bytes.fromhex('85db')); c.rel32(bytes.fromhex('0f84'),'fail_key')
 c.emit(bytes.fromhex('89d8ffc848c1e0044c01e8'))
 c.emit(bytes.fromhex('c7400c01000000'))
 c.emit(bytes.fromhex('4189db41ffcb'))
 serial('text_ready')
 c.emit(bytes.fromhex('0f09'))
 serial('dma')

 # Derive first output stream descriptor from GCAP.ISS.
 c.emit(b'\x41\x0f\xb7\x06\xc1\xe8\x08\x83\xe0\x0f\xc1\xe0\x05\x05\x80\x00\x00\x00')
 c.emit(b'\x49\x8d\x1c\x06')
 # Reset SDCTL with byte accesses so SDSTS at +3 is never overwritten.
 c.emit(b'\x8a\x03\x24\xfd\x88\x03\xb9\xa0\x86\x01\x00')
 c.label('sd_run_clear'); c.emit(b'\xf6\x03\x02'); c.rel32(b'\x0f\x84','sd_run_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sd_run_clear'); c.rel32(b'\xe9','fail_stream')
 c.label('sd_run_clear_ok')
 c.emit(b'\x8a\x03\x0c\x01\x88\x03\xb9\xa0\x86\x01\x00')
 c.label('sd_reset_set'); c.emit(b'\xf6\x03\x01'); c.rel32(b'\x0f\x85','sd_reset_set_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sd_reset_set'); c.rel32(b'\xe9','fail_stream')
 c.label('sd_reset_set_ok')
 c.emit(b'\x8a\x03\x24\xfe\x88\x03\xb9\xa0\x86\x01\x00')
 c.label('sd_reset_clear'); c.emit(b'\xf6\x03\x01'); c.rel32(b'\x0f\x84','sd_reset_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sd_reset_clear'); c.rel32(b'\xe9','fail_stream')
 c.label('sd_reset_clear_ok')
 # CBL, LVI, format, BDL address.
 c.emit(b'\x44\x89\x53\x08')
 c.emit(b'\x66\x44\x89\x5b\x0c')
 c.emit(b'\x66\xc7\x43\x12\x11\x00')
 c.emit(b'\x44\x89\xe8\x89\x43\x18')
 c.emit(b'\x4c\x89\xe8\x48\xc1\xe8\x20\x89\x43\x1c')
 serial('stream')

 # Route the runtime-discovered DAC and output pin.
 verb_data(L['dac_nid'],0x00070610)
 verb_data(L['dac_nid'],0x00020011)
 verb_data(L['pin_nid'],0x00070740)
 serial('codec')

 # Program stream number 1 in SDCTL byte 2, then RUN in SDCTL byte 0.
 c.emit(b'\xc6\x43\x02\x10\xc6\x03\x02')
 # Give HDA backend time to consume DMA, then prove LPIB moved.
 c.emit(b'\xb9\x80\x1a\x06\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x8b\x43\x04\x85\xc0'); c.rel32(b'\x0f\x84','fail_stream')
 serial('progress')
 # Stop stream.
 c.emit(b'\x8a\x03\x24\xfd\x88\x03')
 serial('done')
 c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')

 c.label('fail_hii_protocol'); serial('hii_protocol_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_list_size'); serial('hii_list_size_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_list_fetch'); serial('hii_list_fetch_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_list_fetch_invalid'); serial('hii_list_fetch_invalid'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_list_fetch_not_found'); serial('hii_list_fetch_not_found'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_list_static_small'); serial('hii_list_static_small'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_handle'); serial('hii_handle_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_alloc'); serial('hii_alloc_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_export'); serial('hii_export_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_ifr'); serial('hii_ifr_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_language'); serial('hii_language_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_hii_string'); serial('hii_string_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_no_hda'); serial('no_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_bad_hda'); serial('bad_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_alloc'); serial('alloc'); c.rel32(b'\xe9','return_fail')
 c.label('fail_verb'); serial('verb'); c.rel32(b'\xe9','return_fail')
 c.label('fail_stream'); serial('stream_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_key'); serial('key_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_topology'); serial('topology_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_policy'); serial('policy_fail')
 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return')
 c.emit(b'\x48\x83\xc4\x68\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5f\x5e\x5d\x5b\xc3')

 # Immediate Command helper, MMIO base r14, command eax.
 c.label('immediate')
 c.emit(b'\x41\x89\xc0\xb9\xa0\x86\x01\x00')
 c.label('ic_ready_poll')
 c.emit(b'\x41\x0f\xb7\x56\x68\xf6\xc2\x01'); c.rel32(b'\x0f\x84','ic_ready')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','ic_ready_poll'); c.rel32(b'\xe9','ic_timeout')
 c.label('ic_ready')
 c.emit(b'\x66\x41\xc7\x46\x68\x02\x00')
 c.emit(b'\x45\x89\x46\x60')
 c.emit(b'\x66\x41\xc7\x46\x68\x01\x00')
 c.emit(b'\xb9\xa0\x86\x01\x00')
 c.label('irv_poll')
 c.emit(b'\x41\x0f\xb7\x56\x68\xf6\xc2\x02'); c.rel32(b'\x0f\x85','irv_ready')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','irv_poll')
 c.label('ic_timeout'); c.emit(b'\xb8\xff\xff\xff\xff\xc3')
 c.label('irv_ready')
 c.emit(b'\x41\x8b\x46\x64')
 c.emit(b'\x66\x41\xc7\x46\x68\x02\x00\xc3')

 c.label('pci_read32'); c.emit(b'\x66\xba\xf8\x0c\xef\x66\xba\xfc\x0c\xed\xc3')
 c.label('pci_write32'); c.emit(b'\x66\xba\xf8\x0c\xef\x89\xc8\x66\xba\xfc\x0c\xef\xc3')
 c.label('serial_emit')
 c.emit(b'\x49\x89\xd0\x66\xba\xfd\x03')
 c.label('serial_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'serial_wait')
 c.emit(b'\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9'); c.rel8(0x75,'serial_wait'); c.emit(b'\xc3')
 c.patch(); code=bytes(c.data)
 if len(code)>0x7000: raise SystemExit('HII speech code too large')

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
 return bytes(image),pcm

def validate(image,pcm):
 assert image[:2]==b'MZ'
 pe=struct.unpack_from('<I',image,0x3c)[0]; assert image[pe:pe+4]==b'PE\0\0'
 coff=pe+4; assert struct.unpack_from('<HH',image,coff)==(0x8664,3)
 opt=coff+20; assert struct.unpack_from('<H',image,opt)[0]==0x20b; assert struct.unpack_from('<H',image,opt+0x44)[0]==10
 sec=opt+0xf0; ds=sec+40; tc=struct.unpack_from('<I',image,sec+0x24)[0]; dc=struct.unpack_from('<I',image,ds+0x24)[0]
 assert not(tc&0x80000000); assert dc&0x80000000 and not(dc&0x20000000)
 assert pcm in image
 for m in MARKS.values(): assert m in image

def main():
 if len(sys.argv)!=2: raise SystemExit('usage: build_uefi_hii_speech.py OUTPUT_EFI')
 image,pcm=build(); validate(image,pcm)
 p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HII_SPEECH_BUILD=PASS')
 print('bytes='+str(len(image)))
 print('unit-bank-order='+','.join(UNIT_LAYOUT))
 print('hii-title-graphemes=a-z')
 print('hii-title-max-spoken-graphemes=8')
 print('full-utterance-pcm-assets=0')
 print('pcm-bytes='+str(len(pcm)))
 print('pcm-sha256='+hashlib.sha256(pcm).hexdigest())
 print('sha256='+hashlib.sha256(image).hexdigest())
if __name__=='__main__': main()
