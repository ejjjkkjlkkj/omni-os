#!/usr/bin/env python3
from __future__ import annotations
import hashlib, importlib.util, struct, sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'boot'/'uefi-native-speech-v1'/'build_uefi_native_speech.py'
LETTER_UNITS={
 # Clear fallback spelling for arbitrary firmware labels. The previous map
 # treated each grapheme as a raw phoneme, which made unknown HII labels sound
 # like fused pseudo-words. Here each grapheme expands to a short French letter
 # name; the EFI runtime already inserts a gap between graphemes and can
 # interrupt the DMA stream immediately when focus moves.
 'a':('a',),
 'b':('b','e'),
 'c':('s','e'),
 'd':('d','e'),
 'e':('e',),
 'f':('e','f'),
 'g':('zh','e'),
 'h':('a','sh'),
 'i':('i',),
 'j':('zh','i'),
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

def load_source():
    spec=importlib.util.spec_from_file_location('qevarynx_native_speech_source',SOURCE)
    if spec is None or spec.loader is None:
        raise SystemExit('cannot load native speech source')
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

def convert(raw: bytes, source_rate: int) -> bytes:
    if source_rate <= 0 or 48000 % source_rate:
        raise SystemExit(f'unsupported source sample rate: {source_rate}')
    factor=48000//source_rate
    out=bytearray()
    for i,sample in enumerate(raw):
        a=max(-32768,min(32767,(sample-128)*180))
        nxt=raw[i+1] if i+1 < len(raw) else sample
        b=max(-32768,min(32767,(nxt-128)*180))
        for phase in range(factor):
            signed=a + ((b-a)*phase)//factor
            out += struct.pack('<hh',signed,signed)
    return bytes(out)

def arr_u8(name, values, cols=16):
    lines=[]
    for i in range(0,len(values),cols):
        lines.append('    '+', '.join(str(x) for x in values[i:i+cols])+',')
    return f'const unsigned char {name}[] = {{\n'+'\n'.join(lines)+'\n};\n'

def arr_u32(name, values, cols=8):
    lines=[]
    for i in range(0,len(values),cols):
        lines.append('    '+', '.join(str(x)+'u' for x in values[i:i+cols])+',')
    return f'const unsigned int {name}[] = {{\n'+'\n'.join(lines)+'\n};\n'

def main():
    if len(sys.argv)!=3:
        raise SystemExit('usage: generate_units.py OUTPUT_C METADATA')
    out=Path(sys.argv[1]); meta=Path(sys.argv[2])
    speech=load_source()
    names=sorted({'sil'} | {u for seq in LETTER_UNITS.values() for u in seq})
    source_units=speech.make_units()
    converted={n:convert(source_units[n], speech.SAMPLE_RATE) for n in names}
    offsets=[]; lengths=[]; bank=bytearray()
    for n in names:
        offsets.append(len(bank)); lengths.append(len(converted[n])); bank += converted[n]
    if len(bank) > 128*4096-0x1000:
        raise SystemExit(f'unit bank too large: {len(bank)}')
    index={n:i for i,n in enumerate(names)}
    counts=[]; flat=[]
    for ch in 'abcdefghijklmnopqrstuvwxyz':
        seq=LETTER_UNITS[ch]
        if len(seq)>8: raise SystemExit('letter unit fanout too large')
        counts.append(len(seq))
        row=[index[u] for u in seq] + [0]*(8-len(seq))
        flat.extend(row)
    lines=[
        '/* Generated deterministically from first-party native speech units. */',
        arr_u8('qev_unit_bank',list(bank)),
        f'const unsigned int qev_unit_bank_len = {len(bank)}u;\n',
        arr_u32('qev_unit_off',offsets),
        arr_u32('qev_unit_len',lengths),
        f'const unsigned int qev_unit_count = {len(names)}u;\n',
        f'const unsigned int qev_sil_unit_index = {index["sil"]}u;\n',
        arr_u8('qev_letter_unit_count',counts),
        arr_u8('qev_letter_units',flat),
    ]
    out.write_text('\n'.join(lines))
    meta.write_text(
        'OS-UEFI-HII-GRAPH-PROMPT-SPEECH-UNITS-V1\n'
        'source=boot/uefi-native-speech-v1/build_uefi_native_speech.py\n'
        f'source-sha256={hashlib.sha256(SOURCE.read_bytes()).hexdigest()}\n'
        f'unit-names={",".join(names)}\n'
        f'unit-count={len(names)}\n'
        f'bank-bytes={len(bank)}\n'
        f'bank-sha256={hashlib.sha256(bank).hexdigest()}\n'
        'letter-map=a-z-french-letter-names\n'
        'max-input-graphemes=32\n'
        'max-units-per-letter=8\n'
        'inter-letter-silence-ms=12-runtime-gap\n'
        'word-silence-ms=65\n'
        'speech-mode=clear-lettername-spelling-fr-v3\n'
        'full-utterance-asset=false\n'
    )
    print('HII_GRAPH_PROMPT_UNIT_GENERATION=PASS')
    print('UNIT_COUNT='+str(len(names)))
    print('BANK_BYTES='+str(len(bank)))

if __name__=='__main__':
    main()
