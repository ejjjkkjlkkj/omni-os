#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import struct
import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'boot'/'uefi-native-speech-v1'/'build_uefi_native_speech.py'

def load_source():
    spec=importlib.util.spec_from_file_location('qevarynx_native_speech_source',SOURCE)
    if spec is None or spec.loader is None:
        raise SystemExit('cannot load native speech source')
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

def convert(raw: bytes) -> bytes:
    out=bytearray()
    for sample in raw:
        signed=max(-32768,min(32767,(sample-128)*180))
        frame=struct.pack('<hh',signed,signed)
        out += frame*6
    return bytes(out)

def c_array(name: str, data: bytes) -> str:
    lines=[]
    for i in range(0,len(data),16):
        chunk=', '.join(f'0x{x:02x}' for x in data[i:i+16])
        lines.append('    '+chunk+',')
    body='\n'.join(lines)
    return (
        f'const unsigned char {name}[] = {{\n{body}\n}};\n'
        f'const unsigned int {name}_len = {len(data)}u;\n'
    )

def main() -> int:
    if len(sys.argv)!=3:
        raise SystemExit('usage: generate_speech_units.py OUTPUT_C METADATA')
    out_c=Path(sys.argv[1])
    metadata=Path(sys.argv[2])
    speech=load_source()
    sequence=tuple(speech.PHRASES['help'])
    if sequence != ('e','d'):
        raise SystemExit(f'unexpected help sequence: {sequence!r}')
    units=speech.make_units()
    converted={name:convert(units[name]) for name in ('d','e')}
    bank=converted['d']+converted['e']
    utterance=converted['e']+converted['d']
    if bank == utterance:
        raise SystemExit('unit bank accidentally equals runtime utterance')

    text=(
        '/* Generated deterministically from first-party allophone source. */\n'
        + c_array('qev_unit_d',converted['d'])
        + '\n'
        + c_array('qev_unit_e',converted['e'])
    )
    out_c.write_text(text)
    metadata.write_text(
        'OS-UEFI-HDA-GRAPH-SPEECH-UNITS-V1\n'
        'source=boot/uefi-native-speech-v1/build_uefi_native_speech.py\n'
        f'source-sha256={hashlib.sha256(SOURCE.read_bytes()).hexdigest()}\n'
        'word=aide\n'
        'runtime-sequence=e,d\n'
        'unit-bank-order=d,e\n'
        'full-utterance-asset=false\n'
        f'd-pcm-bytes={len(converted["d"])}\n'
        f'e-pcm-bytes={len(converted["e"])}\n'
        f'd-pcm-sha256={hashlib.sha256(converted["d"]).hexdigest()}\n'
        f'e-pcm-sha256={hashlib.sha256(converted["e"]).hexdigest()}\n'
        f'bank-sha256={hashlib.sha256(bank).hexdigest()}\n'
        f'runtime-utterance-sha256={hashlib.sha256(utterance).hexdigest()}\n'
    )
    print('GRAPH_SPEECH_UNIT_GENERATION=PASS')
    print(f'D_PCM_BYTES={len(converted["d"])}')
    print(f'E_PCM_BYTES={len(converted["e"])}')
    return 0

if __name__=='__main__':
    raise SystemExit(main())
