#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEECH_BUILDER = ROOT / "boot" / "uefi-native-speech-v1" / "build_uefi_native_speech.py"
PLAYBACK_BUILDER = ROOT / "boot" / "uefi-hda-playback-v1" / "build_uefi_hda_playback.py"

MARKS = {
    "start": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATE=START\r\nSYNTH=ALLOPHONE_CONCATENATIVE_V1\r\nWORD=AIDE\r\nTRANSPORT=HDA_NATIVE_DMA\r\nEND\r\n",
    "controller": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nHDA_CONTROLLER_CODEC=PASS\r\nEND\r\n",
    "dma": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nDMA_PAGES=PASS\r\nBDL=PASS\r\nEND\r\n",
    "codec": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nCODEC_DAC_STREAM=PASS\r\nCODEC_PIN_OUTPUT=PASS\r\nEND\r\n",
    "stream": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nOUTPUT_STREAM_DESCRIPTOR=PASS\r\nFORMAT_48K_S16_STEREO=PASS\r\nEND\r\n",
    "progress": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nLPIB_PROGRESS=PASS\r\nHDA_SPEECH_PCM=PASS\r\nEND\r\n",
    "done": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATUS=PASS\r\nEND\r\n",
    "no_hda": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n",
    "bad_hda": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CONTROLLER_OR_CODEC_FAILED\r\nEND\r\n",
    "alloc": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=DMA_ALLOC_FAILED\r\nEND\r\n",
    "verb": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=CODEC_VERB_FAILED\r\nEND\r\n",
    "stream_fail": b"QEVARYNOX-UEFI-HDA-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=STREAM_DMA_NO_PROGRESS\r\nEND\r\n",
}

def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

speech = load_module("qevarynx_native_speech_v1", SPEECH_BUILDER)
play = load_module("qevarynx_hda_playback_v1", PLAYBACK_BUILDER)

def make_pcm() -> bytes:
    units = speech.make_units()
    sequence = speech.PHRASES["help"]
    if tuple(sequence) != ("e", "d"):
        raise SystemExit(f"unexpected AIDE sequence: {sequence!r}")

    source = b"".join(units[name] for name in sequence)
    if not source:
        raise SystemExit("empty speech source")

    # Deterministic first-party conversion: 8 kHz unsigned mono -> 48 kHz
    # signed-16 stereo. Six sample-hold copies preserve the native unit timing.
    out = bytearray()
    for sample in source:
        signed = max(-32768, min(32767, (sample - 128) * 180))
        frame = struct.pack("<hh", signed, signed)
        out += frame * 6

    capacity = play.DMA_PAGES * 4096 - 0x1000
    if len(out) > capacity:
        raise SystemExit(f"speech PCM exceeds DMA allocation: {len(out)} > {capacity}")
    return bytes(out)

def build() -> tuple[bytes, bytes]:
    play.MARKS = MARKS
    play.make_pcm = make_pcm
    image, pcm = play.build()
    play.validate(image, pcm)
    return image, pcm

def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_uefi_hda_speech.py OUTPUT_EFI")
    image, pcm = build()
    out = Path(sys.argv[1])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print("OS_UEFI_HDA_SPEECH_BUILD=PASS")
    print("speech-word=AIDE")
    print("speech-units=e,d")
    print("pcm-format=48000-Hz-s16-stereo")
    print("pcm-bytes=" + str(len(pcm)))
    print("pcm-sha256=" + hashlib.sha256(pcm).hexdigest())
    print("bytes=" + str(len(image)))
    print("sha256=" + hashlib.sha256(image).hexdigest())

if __name__ == "__main__":
    main()
