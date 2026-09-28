#!/usr/bin/env python3
from __future__ import annotations
import hashlib, importlib.util, math, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x4000
RATE=48000
CHANNELS=2
BITS=16
TONE_HZ=660.0
TONE_MS=320
DMA_PAGES=16
PCM_OFF=0x1000

MARKS={
 'start': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATE=START\r\nSYNTH=ALLOPHONE_BDL_RUNTIME_V1\r\nWORD=AIDE\r\nUNITS=e,d\r\nROUTE=AUTO_DIRECT\r\nTRANSPORT=HDA_NATIVE_DMA\r\nEND\r\n',
 'controller': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nHDA_CONTROLLER_CODEC=PASS\r\nEND\r\n',
 'topology': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nAFG_RUNTIME_DISCOVERY=PASS\r\nAUTO_DAC_WIDGET=PASS\r\nAUTO_OUTPUT_PIN_WIDGET=PASS\r\nAUTO_PIN_TO_DAC_DIRECT_ROUTE=PASS\r\nNO_FIXED_WIDGET_NIDS=PASS\r\nEND\r\n',
 'policy': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nPIN_CONFIG_DEFAULT=PASS\r\nDAC_OUTPUT_AMP_VERIFY=PASS\r\nEND\r\n',
 'dma': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nUNIT_BANK_COPY=PASS\r\nBDL_ALLOPHONE_SCHEDULE=PASS\r\nEND\r\n',
 'codec': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nCODEC_DAC_STREAM=PASS\r\nCODEC_PIN_OUTPUT=PASS\r\nEND\r\n',
 'stream': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nOUTPUT_STREAM_DESCRIPTOR=PASS\r\nFORMAT_48K_S16_STEREO=PASS\r\nBDL_ENTRIES=2\r\nEND\r\n',
 'progress': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nLPIB_PROGRESS=PASS\r\nRUNTIME_ALLOPHONE_HDA=PASS\r\nEND\r\n',
 'done': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=PASS\r\nEND\r\n',
 'no_hda': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n',
 'bad_hda': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CONTROLLER_OR_CODEC_FAILED\r\nEND\r\n',
 'alloc': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=DMA_ALLOC_FAILED\r\nEND\r\n',
 'verb': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=CODEC_VERB_FAILED\r\nEND\r\n',
 'stream_fail': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=STREAM_DMA_NO_PROGRESS\r\nEND\r\n',
 'topology_fail': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=AUTO_OUTPUT_ROUTE_DISCOVERY_FAILED\r\nEND\r\n',
 'policy_fail': b'QEVARYNOX-UEFI-HDA-SPEECH-AUTOROUTE-V1\r\nSTATUS=BLOCKED\r\nREASON=OUTPUT_POLICY_FAILED\r\nEND\r\n',
}

ROOT=Path(__file__).resolve().parents[2]
SPEECH_BUILDER=ROOT/'boot'/'uefi-native-speech-v1'/'build_uefi_native_speech.py'
RUNTIME_SEQUENCE=('e','d')
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
 expected=tuple(speech.PHRASES['help'])
 if expected != RUNTIME_SEQUENCE:
  raise SystemExit(f'unexpected AIDE source sequence: {expected!r}')
 bank_names=sorted(set(RUNTIME_SEQUENCE))
 converted={name:convert_unit(units[name]) for name in bank_names}
 bank=bytearray()
 UNIT_LAYOUT.clear()
 for name in bank_names:
  off=len(bank); raw=converted[name]
  UNIT_LAYOUT[name]=(off,len(raw)); bank += raw
 utterance=b''.join(converted[name] for name in RUNTIME_SEQUENCE)
 if bytes(bank)==utterance:
  raise SystemExit('unit bank accidentally equals pre-concatenated utterance')
 capacity=DMA_PAGES*4096-PCM_OFF
 if len(bank)>capacity:
  raise SystemExit(f'unit bank exceeds DMA allocation: {len(bank)} > {capacity}')
 return bytes(bank)
def build():
 pcm=make_pcm()
 data=bytearray(0x100)
 L={'maxaddr':0,'dac_nid':8,'pin_nid':12}
 struct.pack_into('<Q',data,0,0xffffffff)
 for n,m in MARKS.items(): L[n]=len(data); data+=m
 L['pcm']=len(data); data+=pcm

 c=Code()
 # Preserve all nonvolatile registers touched by the raw EFI entry point.
 c.emit(b'\x53\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57')
 c.emit(b'\x4c\x8b\x7a\x60')  # r15 = BootServices
 c.emit(b'\x48\x83\xec\x20\xfc')
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
  # Build a codec verb from a runtime-discovered NID stored in image data.
  c.emit(b'\x44\x89\xe0\xc1\xe0\x1c')
  c.lea_rdx_data(nid_off)
  c.emit(b'\x8b\x0a\xc1\xe1\x14\x09\xc8')
  c.emit(b'\x0d'+struct.pack('<I',value))
  c.rel32(b'\xe8','immediate')
  c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb')

 serial('start')
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

 # Discover the Audio Function Group from root subordinate nodes.
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

 # Discover the AFG widget range.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x04\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x41\x89\xc5\x41\x81\xe5\xff\x00\x00\x00\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc6\xc1\xee\x10\x81\xe6\xff\x00\x00\x00\x85\xf6'); c.rel32(b'\x0f\x84','fail_topology')

 # r10d = discovered Audio Output converter NID; r11d = output-capable Pin NID.
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

 # Verify a direct connection from the discovered output pin to the discovered DAC.
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

 # Persist the discovered NIDs across BootServices calls.
 c.lea_rdx_data(L['dac_nid']); c.emit(b'\x44\x89\x12')
 c.lea_rdx_data(L['pin_nid']); c.emit(b'\x44\x89\x1a')
 serial('topology')

 # Query output-pin Config Default and require analog line-out/speaker/headphone.
 verb_data(L['pin_nid'],0x000f1c00)
 c.emit(b'\xc1\xe8\x14\x83\xe0\x0f\x83\xf8\x02'); c.rel32(b'\x0f\x87','fail_policy')

 # Require DAC output amplifier capabilities, program gain 0x40, verify left+right.
 verb_data(L['dac_nid'],0x000f0012)
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x84','fail_policy')
 verb_data(L['dac_nid'],0x0003b040)
 verb_data(L['dac_nid'],0x000ba000)
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x40'); c.rel32(b'\x0f\x85','fail_policy')
 verb_data(L['dac_nid'],0x000b8000)
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x40'); c.rel32(b'\x0f\x85','fail_policy')
 serial('policy')

 # Allocate 64 KiB below 4 GiB: BDL at base, PCM at base+0x1000.
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
 # Runtime HDA BDL scheduling: descriptors replay e -> d even though the
 # embedded unit bank is sorted d -> e. No full-utterance PCM asset exists.
 total=0
 for idx,name in enumerate(RUNTIME_SEQUENCE):
  unit_off,unit_len=UNIT_LAYOUT[name]
  desc=idx*16
  c.emit(b'\x49\x8d\x85'+struct.pack('<i',PCM_OFF+unit_off))
  c.emit(b'\x49\x89\x85'+struct.pack('<i',desc))
  c.emit(b'\x41\xc7\x85'+struct.pack('<i',desc+8)+struct.pack('<I',unit_len))
  flags=1 if idx==len(RUNTIME_SEQUENCE)-1 else 0
  c.emit(b'\x41\xc7\x85'+struct.pack('<i',desc+12)+struct.pack('<I',flags))
  total += unit_len
 c.emit(b'\x0f\x09')
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
 c.emit(b'\xc7\x43\x08'+struct.pack('<I',total))
 c.emit(b'\x66\xc7\x43\x0c'+struct.pack('<H',len(RUNTIME_SEQUENCE)-1))
 c.emit(b'\x66\xc7\x43\x12\x11\x00')
 c.emit(b'\x44\x89\xe8\x89\x43\x18')
 c.emit(b'\x4c\x89\xe8\x48\xc1\xe8\x20\x89\x43\x1c')
 serial('stream')

 # Route the dynamically discovered DAC and output pin.
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

 c.label('fail_no_hda'); serial('no_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_bad_hda'); serial('bad_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_alloc'); serial('alloc'); c.rel32(b'\xe9','return_fail')
 c.label('fail_verb'); serial('verb'); c.rel32(b'\xe9','return_fail')
 c.label('fail_stream'); serial('stream_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_topology'); serial('topology_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_policy'); serial('policy_fail')
 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return')
 c.emit(b'\x48\x83\xc4\x20\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5f\x5e\x5b\xc3')

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
 if len(code)>0x3000: raise SystemExit('text too large')

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
 if len(sys.argv)!=2: raise SystemExit('usage: build_uefi_hda_speech_autoroute.py OUTPUT_EFI')
 image,pcm=build(); validate(image,pcm)
 p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HDA_SPEECH_AUTOROUTE_BUILD=PASS')
 print('bytes='+str(len(image)))
 print('unit-bank-order=d,e')
 print('runtime-sequence=e,d')
 print('full-utterance-pcm-assets=0')
 print('pcm-bytes='+str(len(pcm)))
 print('pcm-sha256='+hashlib.sha256(pcm).hexdigest())
 print('sha256='+hashlib.sha256(image).hexdigest())
if __name__=='__main__': main()
