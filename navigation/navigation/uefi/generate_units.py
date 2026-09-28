#!/usr/bin/env python3
from __future__ import annotations
import hashlib, importlib.util, os, struct, sys, wave
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'voice'/'v4'/'native_speech_v4.py'
SOURCE_RATE=16000 if os.environ.get('QEV_EXTERNAL_VOICE_DIR','').strip() else 24000
UNIT_ENCODING_MULAW=1
UEFI_VOICE_PROFILE='clair'
MAX_BANK_BYTES=1200*1024
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

# BIOS labels contain semantic digits everywhere: IPv4/IPv6, USB 3, TPM 2.0,
# Boot Option #1, BIOS versions and timeout values. Preserve them instead of
# dropping them during prompt normalization.
DIGIT_UNITS={
 '0':('z','e','r','o'),
 '1':('eu','n'),
 '2':('d','eu'),
 '3':('t','r','w','a'),
 '4':('k','a','t','r'),
 '5':('s','e','n','k'),
 '6':('s','i','s'),
 '7':('s','e','p','t'),
 '8':('w','i','t'),
 '9':('n','eu','f'),
}

# Compact semantic pronunciation lexicon for recurring firmware words. These
# entries reuse the same first-party allophone bank, so they add negligible
# runtime footprint compared with pre-rendered whole-word PCM. Unknown words
# still fall back to deterministic French letter-name spelling.
WORD_UNITS={
 'advanced':('a','d','v','a','n','s','t'),
 'access':('a','k','s','e','s'),
 'action':('a','k','sh','on'),
 'asus':('a','s','u','s'),
 'back':('b','a','k'),
 'bios':('b','i','o','s'),
 'button':('b','a','t','on'),
 'change':('sh','e','n','zh'),
 'checked':('sh','e','k','t'),
 'choice':('sh','o','i','s'),
 'conditional':('k','on','d','i','sh','on','a','l'),
 'cpu':('s','e','p','e','u'),
 'details':('d','i','t','e','l','s'),
 'down':('d','a','w','n'),
 'editable':('e','d','i','t','a','b','l'),
 'edits':('e','d','i','t','s'),
 'enter':('e','n','t','e','r'),
 'escape':('e','s','k','e','p'),
 'firmware':('f','e','r','m','w','e','r'),
 'flash':('f','l','a','sh'),
 'for':('f','o','r'),
 'help':('e','l','p'),
 'left':('l','e','f','t'),
 'main':('m','e','n'),
 'move':('m','u','v'),
 'no':('n','o'),
 'not':('n','o','t'),
 'nvme':('e','n','v','e','e','m','e'),
 'only':('o','n','l','i'),
 'pending':('p','e','n','d','i','n','g'),
 'position':('p','o','z','i','s','i','on'),
 'press':('p','r','e','s'),
 'preview':('p','r','i','v','i','u'),
 'ready':('r','e','d','i'),
 'right':('r','i','t'),
 'sata':('s','a','t','a'),
 'setup':('s','e','t','u','p'),
 'smart':('s','m','a','r','t'),
 'stack':('s','t','a','k'),
 'tpm':('t','e','p','e','e','m'),
 'up':('a','p'),
 'usb':('u','e','s','b','e'),
 'value':('v','a','l','u'),
 'administrator':('a','d','m','i','n','i','s','t','r','a','t','o','r'),
 'boot':('b','u','t'),
 'changes':('sh','e','n','zh','e','s'),
 'configuration':('k','on','f','i','g','u','r','a','s','i','on'),
 'control':('k','on','t','r','o','l'),
 'default':('d','e','f','o','l','t'),
 'delete':('d','i','l','i','t'),
 'device':('d','i','v','a','i','s'),
 'disabled':('d','i','s','e','b','l','d'),
 'discard':('d','i','s','k','a','r','d'),
 'enabled':('e','n','e','b','l','d'),
 'exit':('e','k','s','i','t'),
 'fast':('f','a','s','t'),
 'information':('i','n','f','o','r','m','a','s','i','on'),
 'interface':('i','n','t','e','r','f','e','s'),
 'internal':('i','n','t','e','r','n','a','l'),
 'management':('m','a','n','a','zh','m','e','n','t'),
 'memory':('m','e','m','o','r','i'),
 'mode':('m','o','d'),
 'network':('n','e','t','w','o','r','k'),
 'open':('o','p','e','n'),
 'option':('o','p','s','i','on'),
 'password':('p','a','s','w','o','r','d'),
 'priority':('p','r','i','o','r','i','t','i'),
 'processor':('p','r','o','s','e','s','o','r'),
 'recovery':('r','i','k','a','v','e','r','i'),
 'restore':('r','e','s','t','o','r'),
 'save':('s','e','v'),
 'secure':('s','e','k','u','r'),
 'security':('s','i','k','u','r','i','t','i'),
 'settings':('s','e','t','i','n','g','s'),
 'storage':('s','t','o','r','a','zh'),
 'support':('s','u','p','o','r','t'),
 'system':('s','i','s','t','e','m'),
 'trusted':('t','r','u','s','t','e','d'),
 'user':('u','z','e','r'),
 'utility':('u','t','i','l','i','t','i'),
 'version':('v','e','r','zh','on'),
 'wake':('w','e','k'),
}

# Preserve the compact pronunciation sequences, but render each complete
# letter/digit/word through VoiceCore v4 before embedding it in firmware.
# This keeps coarticulation/crossfades inside words instead of restarting the
# oscillator/filter for every phoneme.
PHONEME_LETTER_UNITS = LETTER_UNITS
PHONEME_DIGIT_UNITS = DIGIT_UNITS
PHONEME_WORD_UNITS = WORD_UNITS

LETTER_UNITS = {ch:(f'letter_{ch}',) for ch in PHONEME_LETTER_UNITS}
DIGIT_UNITS = {ch:(f'digit_{ch}',) for ch in PHONEME_DIGIT_UNITS}
WORD_UNITS = {word:(f'word_{word}',) for word in PHONEME_WORD_UNITS}

PHRASE_TEXTS = (
    "ready press f1 for help",
    "up down move left right change",
    "enter action escape back f1 help",
    "no change",
    "preview edits discarded",
    "checked",
    "not checked",
    "protected",
)
PHRASE_NAME_STRIDE=40

def _phrase_unit_name(index):
    return f'phrase_{index}'


_V4_PHONEME = {
    'eu':'ø',
    'on':'ɔ̃',
    'sh':'ʃ',
    'zh':'ʒ',
    'r':'ʁ',
}

def _render_sequence(speech, sequence):
    mapped=[_V4_PHONEME.get(p,p) for p in sequence]
    if not mapped:
        return []
    voice=speech.VOICES[UEFI_VOICE_PROFILE]
    out=[]
    for idx,ph in enumerate(mapped):
        if ph not in speech.PHONEMES:
            raise SystemExit(f'VoiceCore v4 missing phoneme: {ph}')
        seg=speech._segment(ph, voice, idx, len(mapped), '')
        kind=speech.PHONEMES[ph].kind
        out=speech._crossfade(out, seg, 2.0 if kind in {'p','s'} else 7.0)
    return speech._finalize(out)

MULAW_BIAS=0x84
MULAW_CLIP=32635

def _linear_to_mulaw(sample):
    sample=max(-32768,min(32767,int(sample)))
    sign=0x80 if sample < 0 else 0
    if sample < 0:
        sample=-sample
    sample=min(MULAW_CLIP,sample)+MULAW_BIAS
    exponent=7
    mask=0x4000
    while exponent > 0 and not (sample & mask):
        exponent-=1
        mask >>= 1
    mantissa=(sample >> (exponent+3)) & 0x0f
    return (~(sign | (exponent << 4) | mantissa)) & 0xff

def _mulaw_to_linear(code):
    u=(~int(code)) & 0xff
    exponent=(u >> 4) & 0x07
    mantissa=u & 0x0f
    sample=((mantissa << 3) + MULAW_BIAS) << exponent
    sample-=MULAW_BIAS
    return -sample if (u & 0x80) else sample

def _to_mulaw_source_rate(samples):
    # VoiceCore renders 48 kHz signed-16 mono. Hardware real-voice builds use
    # 16 kHz mu-law to reserve firmware space for complete SAPI phrases; normal
    # deterministic CI keeps 24 kHz. Both divide 48 kHz exactly.
    if SOURCE_RATE <= 0 or 48000 % SOURCE_RATE:
        raise SystemExit(f'unsupported source sample rate: {SOURCE_RATE}')
    factor=48000//SOURCE_RATE
    out=bytearray()
    for i in range(0,len(samples)-factor+1,factor):
        avg=sum(int(samples[i+j]) for j in range(factor))//factor
        out.append(_linear_to_mulaw(avg))
    return bytes(out)

def _wav_to_mulaw_source_rate(path: Path) -> bytes:
    with wave.open(str(path), 'rb') as w:
        channels=w.getnchannels()
        width=w.getsampwidth()
        rate=w.getframerate()
        frames=w.getnframes()
        comptype=w.getcomptype()
        # Real SYSTEM speech is generated in the exact bank format. Reject any
        # implicit host conversion here: double resampling was a major source
        # of metallic/garbled consonants on the physical HDA path.
        if channels != 1 or width != 2 or rate != SOURCE_RATE or comptype != 'NONE':
            raise SystemExit(
                f'real-voice WAV must be PCM mono 16-bit {SOURCE_RATE} Hz: '
                f'{path} (channels={channels}, width={width}, rate={rate}, '
                f'compression={comptype})'
            )
        raw=w.readframes(frames)
    mono=[int(x) for x in struct.unpack('<' + 'h'*(len(raw)//2), raw)]
    if not mono:
        raise SystemExit(f'empty real-voice WAV: {path}')

    # Remove DC bias before companding. A biased waveform wastes mu-law range
    # and makes quiet consonants harder to distinguish.
    dc=sum(mono)//len(mono)
    mono=[max(-32768,min(32767,x-dc)) for x in mono]

    # Remove excessive SAPI lead/tail silence while keeping 25 ms safety pads.
    peak=max(abs(x) for x in mono)
    if peak < 256:
        raise SystemExit(f'real-voice WAV has no usable speech level: {path}')
    threshold=max(160, peak//120)
    first=0
    while first < len(mono) and abs(mono[first]) < threshold:
        first+=1
    last=len(mono)
    while last > first and abs(mono[last-1]) < threshold:
        last-=1
    pad=max(1, rate*25//1000)
    first=max(0, first-pad)
    last=min(len(mono), last+pad)
    mono=mono[first:last] if first < last else mono

    # Keep headroom while lifting weak SYSTEM voices before G.711 mu-law.
    # 28k avoids hard clipping on interpolation/codec paths but uses most of
    # the signed-16 dynamic range.
    peak=max(abs(x) for x in mono)
    target_peak=28000
    if 0 < peak < target_peak:
        mono=[max(-32768,min(32767,(x*target_peak)//peak)) for x in mono]

    # A short fade suppresses clicks caused by hard clipping at trim edges.
    fade=max(1, rate*2//1000)
    fade=min(fade,len(mono)//2)
    for i in range(fade):
        mono[i]=(mono[i]*i)//fade
        j=len(mono)-1-i
        mono[j]=(mono[j]*i)//fade

    return bytes(_linear_to_mulaw(x) for x in mono)

def _external_unit_order(names):
    phrases=[_phrase_unit_name(i) for i in range(len(PHRASE_TEXTS))]
    letters=[f'letter_{ch}' for ch in 'abcdefghijklmnopqrstuvwxyz']
    digits=[f'digit_{ch}' for ch in '0123456789']
    words=[
        # Highest-value ASUS/AMI setup tabs and commit actions first. Keeping
        # these as complete System.Speech clips avoids synthetic fallback in
        # the screens a blind user must traverse most often.
        'word_main','word_advanced','word_boot','word_security','word_save',
        'word_exit','word_setup','word_bios','word_system','word_settings',
        'word_enabled','word_disabled','word_option','word_value','word_device',
        'word_storage','word_usb','word_nvme','word_tpm','word_cpu','word_memory',
        'word_network','word_password','word_secure','word_configuration',
        'word_processor','word_recovery','word_restore','word_default',
        'word_enter','word_escape','word_help','word_up','word_down','word_left',
        'word_right','word_change','word_action','word_checked','word_back',
        'word_button',
    ]
    # Physical intelligibility priority: keep complete guidance phrases
    # first, then common BIOS words as whole clips. Letters remain the final
    # fallback for unknown labels. The previous phrases+letters+digits+words
    # order exhausted the 1.2 MiB bank before any real whole-word clips could
    # be accepted, forcing navigation labels back to synthetic VoiceCore.
    ordered=phrases+words+digits+letters
    return [n for n in ordered if n in names]

def apply_external_voice_units(units, names):
    root=os.environ.get('QEV_EXTERNAL_VOICE_DIR','').strip()
    if not root:
        return units, [], []
    voice_dir=Path(root)
    if not voice_dir.is_dir():
        raise SystemExit(f'QEV_EXTERNAL_VOICE_DIR not found: {voice_dir}')

    result=dict(units)
    current=sum(len(result[n]) for n in names)
    accepted=[]
    skipped=[]
    for name in _external_unit_order(names):
        path=voice_dir/(name+'.wav')
        if not path.is_file():
            continue
        encoded=_wav_to_mulaw_source_rate(path)
        projected=current-len(result[name])+len(encoded)
        if projected > MAX_BANK_BYTES:
            skipped.append(name)
            continue
        current=projected
        result[name]=encoded
        accepted.append(name)
    if not accepted:
        raise SystemExit('real-voice directory present but no WAV asset was accepted')
    return result, accepted, skipped

def make_source_units(speech):
    units={'sil':bytes([_linear_to_mulaw(0)])*(SOURCE_RATE*70//1000)}
    for ch,seq in PHONEME_LETTER_UNITS.items():
        units[f'letter_{ch}']=_to_mulaw_source_rate(_render_sequence(speech,seq))
    for ch,seq in PHONEME_DIGIT_UNITS.items():
        units[f'digit_{ch}']=_to_mulaw_source_rate(_render_sequence(speech,seq))
    for word,seq in PHONEME_WORD_UNITS.items():
        units[f'word_{word}']=_to_mulaw_source_rate(_render_sequence(speech,seq))
    for index,phrase in enumerate(PHRASE_TEXTS):
        units[_phrase_unit_name(index)]=_to_mulaw_source_rate(speech.synthesize(phrase, UEFI_VOICE_PROFILE))
    return units

WORD_NAME_STRIDE=16
WORD_UNIT_STRIDE=24

def load_source():
    spec=importlib.util.spec_from_file_location('qevarynx_native_speech_source',SOURCE)
    if spec is None or spec.loader is None:
        raise SystemExit('cannot load native speech source')
    module=importlib.util.module_from_spec(spec)
    sys.modules[spec.name]=module
    spec.loader.exec_module(module)
    return module

def convert(raw: bytes, source_rate: int) -> bytes:
    if source_rate <= 0 or 48000 % source_rate:
        raise SystemExit(f'unsupported source sample rate: {source_rate}')
    factor=48000//source_rate
    out=bytearray()
    for i,sample in enumerate(raw):
        a=_mulaw_to_linear(sample)
        nxt=raw[i+1] if i+1 < len(raw) else sample
        b=_mulaw_to_linear(nxt)
        for phase in range(factor):
            signed=a + ((b-a)*phase)//factor
            signed=max(-32768,min(32767,signed))
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
    if speech.SAMPLE_RATE != 48000:
        raise SystemExit(f'VoiceCore v4 unexpected render rate: {speech.SAMPLE_RATE}')
    names=sorted(
        {'sil'}
        | {u for seq in LETTER_UNITS.values() for u in seq}
        | {u for seq in DIGIT_UNITS.values() for u in seq}
        | {u for seq in WORD_UNITS.values() for u in seq}
        | {_phrase_unit_name(i) for i in range(len(PHRASE_TEXTS))}
    )
    source_units=make_source_units(speech)
    source_units, external_units, skipped_external_units = apply_external_voice_units(source_units, names)
    # Keep compact G.711 mu-law clips in the EFI image. Real-voice hardware
    # builds use 16 kHz to prioritize complete human speech phrases; normal CI
    # uses 24 kHz. Firmware decodes and upsamples to 48 kHz signed-16 stereo.
    converted={n:source_units[n] for n in names}
    offsets=[]; lengths=[]; bank=bytearray()
    for n in names:
        offsets.append(len(bank)); lengths.append(len(converted[n])); bank += converted[n]
    if len(bank) > MAX_BANK_BYTES:
        raise SystemExit(f'unit bank too large: {len(bank)}')
    index={n:i for i,n in enumerate(names)}
    counts=[]; flat=[]
    for ch in 'abcdefghijklmnopqrstuvwxyz':
        seq=LETTER_UNITS[ch]
        if len(seq)>8: raise SystemExit('letter unit fanout too large')
        counts.append(len(seq))
        row=[index[u] for u in seq] + [0]*(8-len(seq))
        flat.extend(row)

    digit_counts=[]; digit_flat=[]
    for ch in '0123456789':
        seq=DIGIT_UNITS[ch]
        if len(seq)>8: raise SystemExit('digit unit fanout too large')
        digit_counts.append(len(seq))
        row=[index[u] for u in seq] + [0]*(8-len(seq))
        digit_flat.extend(row)

    word_names=sorted(WORD_UNITS)
    word_name_lens=[]; word_name_flat=[]
    word_unit_counts=[]; word_unit_flat=[]
    for word in word_names:
        encoded=word.encode('ascii')
        seq=WORD_UNITS[word]
        if len(encoded)>=WORD_NAME_STRIDE:
            raise SystemExit(f'word name too long: {word}')
        if len(seq)>WORD_UNIT_STRIDE:
            raise SystemExit(f'word unit fanout too large: {word}')
        word_name_lens.append(len(encoded))
        word_name_flat.extend(encoded)
        word_name_flat.extend([0]*(WORD_NAME_STRIDE-len(encoded)))
        word_unit_counts.append(len(seq))
        word_unit_flat.extend(index[u] for u in seq)
        word_unit_flat.extend([0]*(WORD_UNIT_STRIDE-len(seq)))

    phrase_name_lens=[]; phrase_name_flat=[]; phrase_unit_indices=[]
    for pi,phrase in enumerate(PHRASE_TEXTS):
        encoded=phrase.encode('ascii')
        if len(encoded)>=PHRASE_NAME_STRIDE:
            raise SystemExit(f'phrase name too long: {phrase}')
        phrase_name_lens.append(len(encoded))
        phrase_name_flat.extend(encoded)
        phrase_name_flat.extend([0]*(PHRASE_NAME_STRIDE-len(encoded)))
        phrase_unit_indices.append(index[_phrase_unit_name(pi)])
    lines=[
        '/* Generated deterministically from first-party native speech units. */',
        arr_u8('qev_unit_bank',list(bank)),
        f'const unsigned int qev_unit_bank_len = {len(bank)}u;\n',
        arr_u32('qev_unit_off',offsets),
        arr_u32('qev_unit_len',lengths),
        f'const unsigned int qev_unit_source_rate = {SOURCE_RATE}u;\n',
        f'const unsigned int qev_unit_encoding = {UNIT_ENCODING_MULAW}u;\n',
        f'const unsigned int qev_unit_count = {len(names)}u;\n',
        f'const unsigned int qev_sil_unit_index = {index["sil"]}u;\n',
        arr_u8('qev_letter_unit_count',counts),
        arr_u8('qev_letter_units',flat),
        arr_u8('qev_digit_unit_count',digit_counts),
        arr_u8('qev_digit_units',digit_flat),
        f'const unsigned int qev_word_count = {len(word_names)}u;\n',
        f'const unsigned int qev_word_name_stride = {WORD_NAME_STRIDE}u;\n',
        f'const unsigned int qev_word_unit_stride = {WORD_UNIT_STRIDE}u;\n',
        arr_u8('qev_word_name_len',word_name_lens),
        arr_u8('qev_word_names',word_name_flat),
        arr_u8('qev_word_unit_count',word_unit_counts),
        arr_u8('qev_word_units',word_unit_flat),
        f'const unsigned int qev_phrase_count = {len(PHRASE_TEXTS)}u;\n',
        f'const unsigned int qev_phrase_name_stride = {PHRASE_NAME_STRIDE}u;\n',
        arr_u8('qev_phrase_name_len',phrase_name_lens),
        arr_u8('qev_phrase_names',phrase_name_flat),
        arr_u32('qev_phrase_unit_index',phrase_unit_indices),
    ]
    out.write_text('\n'.join(lines))
    meta.write_text(
        'OS-UEFI-HII-GRAPH-PROMPT-SPEECH-UNITS-V1\n'
        'source=voice/v4/native_speech_v4.py\n'
        f'source-sha256={hashlib.sha256(SOURCE.read_bytes()).hexdigest()}\n'
        f'unit-names={",".join(names)}\n'
        f'unit-count={len(names)}\n'
        f'bank-bytes={len(bank)}\n'
        f'bank-sha256={hashlib.sha256(bank).hexdigest()}\n'
        'letter-map=a-z-french-letter-names\n'
        'digit-map=0-9-french-number-names\n'
        'max-input-graphemes=64\n'
        'max-units-per-letter=8\n'
        f'word-lexicon-count={len(word_names)}\n'
        f'word-name-stride={WORD_NAME_STRIDE}\n'
        f'word-unit-stride={WORD_UNIT_STRIDE}\n'
        f'phrase-clip-count={len(PHRASE_TEXTS)}\n'
        'source-sample-rate-hz='+str(SOURCE_RATE)+'\n'
        'unit-encoding=g711-mulaw-u8\n'
        'voice-profile='+UEFI_VOICE_PROFILE+'\n'
        'real-voice-source=' + ('windows-system-speech' if external_units else 'none') + '\n'
        'real-voice-unit-count='+str(len(external_units))+'\n'
        'real-voice-units='+','.join(external_units)+'\n'
        'real-voice-skipped-count='+str(len(skipped_external_units))+'\n'
        'real-voice-word-count='+str(sum(1 for n in external_units if n.startswith('word_')))+'\n'
        'real-voice-digit-count='+str(sum(1 for n in external_units if n.startswith('digit_')))+'\n'
        'real-voice-letter-count='+str(sum(1 for n in external_units if n.startswith('letter_')))+'\n'
        'real-voice-priority=phrases,words,digits,letters\n'
        'inter-letter-silence-ms=18-runtime-gap\n'
        'intra-word-phoneme-silence-ms=0\n'
        'word-silence-ms=70\n'
        'speech-mode=' + ('hybrid-system-speech-clean-pcm16k-plus-voicecore-v4-uefi-v12' if external_units else 'whole-phrase-voicecore-v4-uefi-v8-mulaw24k') + '\n'
        'full-utterance-asset=' + ('true' if external_units and all(_phrase_unit_name(i) in external_units for i in range(len(PHRASE_TEXTS))) else 'false') + '\n'
    )
    print('HII_GRAPH_PROMPT_UNIT_GENERATION=PASS')
    print('UNIT_COUNT='+str(len(names)))
    print('BANK_BYTES='+str(len(bank)))
    print('WORD_LEXICON_COUNT='+str(len(word_names)))
    print('PHRASE_CLIP_COUNT='+str(len(PHRASE_TEXTS)))
    print('REAL_VOICE_UNIT_COUNT='+str(len(external_units)))
    print('REAL_VOICE_SKIPPED_COUNT='+str(len(skipped_external_units)))
    print('REAL_VOICE_WORD_COUNT='+str(sum(1 for n in external_units if n.startswith('word_'))))
    print('REAL_VOICE_DIGIT_COUNT='+str(sum(1 for n in external_units if n.startswith('digit_'))))
    print('REAL_VOICE_LETTER_COUNT='+str(sum(1 for n in external_units if n.startswith('letter_'))))
    if external_units:
        print('SYSTEM_SPEECH_REAL_VOICE_BANK=PASS')

if __name__=='__main__':
    main()
