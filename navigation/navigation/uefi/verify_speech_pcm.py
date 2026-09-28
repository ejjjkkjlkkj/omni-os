#!/usr/bin/env python3
"""Prove that captured QEMU PCM begins with the exact UEFI discovery phrase.

This is a deterministic content proof, not an intelligibility test. It rebuilds
the same first-party 48 kHz stereo PCM units used by NAVIGATION.EFI and requires
a bit-identical WAV prefix for the known startup phrase.
"""

from __future__ import annotations

import hashlib
import json
import sys
import wave
from pathlib import Path

import generate_units as gu

DISCOVERY_TEXT = "ready press f1 for help"
LEAD_SILENCE_BYTES = 30 * 192
GRAPHEME_GAP_BYTES = 18 * 192
PHONEME_GAP_BYTES = 0
TAIL_SILENCE_BYTES = 45 * 192


def converted_units() -> dict[str, bytes]:
    speech = gu.load_source()
    names = sorted(
        {"sil"}
        | {u for seq in gu.LETTER_UNITS.values() for u in seq}
        | {u for seq in gu.DIGIT_UNITS.values() for u in seq}
        | {u for seq in gu.WORD_UNITS.values() for u in seq}
        | {gu._phrase_unit_name(i) for i in range(len(gu.PHRASE_TEXTS))}
    )
    source_units = gu.make_source_units(speech)
    return {
        name: gu.convert(source_units[name], gu.SOURCE_RATE)
        for name in names
    }


def render_runtime_pcm(text: str) -> bytes:
    if not text or len(text) > 32:
        raise ValueError("runtime speech text must be 1..32 characters")

    units = converted_units()
    pcm = bytearray(LEAD_SILENCE_BYTES)
    if text in gu.PHRASE_TEXTS:
        pcm += units[gu._phrase_unit_name(gu.PHRASE_TEXTS.index(text))]
        pcm += bytes(TAIL_SILENCE_BYTES)
        padded = (len(pcm) + 127) & ~127
        pcm += bytes(padded - len(pcm))
        return bytes(pcm)
    i = 0
    while i < len(text):
        ch = text[i]
        if ch == " ":
            pcm += units["sil"]
            i += 1
            continue

        if "a" <= ch <= "z" and (i == 0 or text[i - 1] == " "):
            word_length = 1
            while i + word_length < len(text) and text[i + word_length] != " ":
                word_length += 1
            word = text[i : i + word_length]
            sequence = gu.WORD_UNITS.get(word)
            if sequence:
                for index, unit_name in enumerate(sequence):
                    if index:
                        pcm += bytes(PHONEME_GAP_BYTES)
                    pcm += units[unit_name]
                i += word_length
                continue

        if i != 0 and text[i - 1] != " ":
            pcm += bytes(GRAPHEME_GAP_BYTES)

        if "a" <= ch <= "z":
            sequence = gu.LETTER_UNITS[ch]
        elif "0" <= ch <= "9":
            sequence = gu.DIGIT_UNITS[ch]
        else:
            raise ValueError(f"unsupported runtime character: {ch!r}")

        for unit_name in sequence:
            pcm += units[unit_name]
        i += 1

    pcm += bytes(TAIL_SILENCE_BYTES)
    padded = (len(pcm) + 127) & ~127
    pcm += bytes(padded - len(pcm))
    return bytes(pcm)


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(
            "usage: verify_speech_pcm.py CAPTURE.wav [REPORT.json]",
            file=sys.stderr,
        )
        return 2

    wav_path = Path(sys.argv[1])
    report_path = Path(sys.argv[2]) if len(sys.argv) == 3 else None
    expected = render_runtime_pcm(DISCOVERY_TEXT)

    with wave.open(str(wav_path), "rb") as wav:
        params = {
            "channels": wav.getnchannels(),
            "sample_width_bytes": wav.getsampwidth(),
            "sample_rate_hz": wav.getframerate(),
            "compression": wav.getcomptype(),
            "frames": wav.getnframes(),
        }
        raw = wav.readframes(wav.getnframes())

    format_ok = (
        params["channels"] == 2
        and params["sample_width_bytes"] == 2
        and params["sample_rate_hz"] == 48000
        and params["compression"] == "NONE"
    )
    enough = len(raw) >= len(expected)
    actual_prefix = raw[: len(expected)] if enough else raw
    exact = format_ok and enough and actual_prefix == expected

    first_difference = None
    if enough and actual_prefix != expected:
        for index, (actual, wanted) in enumerate(zip(actual_prefix, expected)):
            if actual != wanted:
                first_difference = index
                break

    report = {
        "discovery_text": DISCOVERY_TEXT,
        "scope": "DISCOVERY_PROMPT_ONLY",
        "wav": params,
        "expected_pcm_bytes": len(expected),
        "expected_pcm_frames": len(expected) // 4,
        "expected_sha256": hashlib.sha256(expected).hexdigest(),
        "captured_prefix_sha256": hashlib.sha256(actual_prefix).hexdigest(),
        "format_ok": format_ok,
        "capture_long_enough": enough,
        "bit_identical_prefix": exact,
        "first_difference_byte": first_difference,
        "discovery_speech_content": "PASS" if exact else "FAIL",
        "full_navigation_speech_content": "NOT_ESTABLISHED",
        "intelligibility": "NOT_TESTED",
    }

    if report_path:
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    print(f"DISCOVERY_SPEECH_CONTENT={'PASS' if exact else 'FAIL'}")
    print(f"DISCOVERY_SPEECH_PCM_EXACT={'PASS' if exact else 'FAIL'}")
    print("SPEECH_CONTENT_SCOPE=DISCOVERY_PROMPT_ONLY")
    print("FULL_NAVIGATION_SPEECH_CONTENT=NOT_ESTABLISHED")
    print("INTELLIGIBILITY=NOT_TESTED")
    print(f"DISCOVERY_PCM_BYTES={len(expected)}")
    print(f"DISCOVERY_PCM_FRAMES={len(expected) // 4}")
    print(f"DISCOVERY_PCM_SHA256={report['expected_sha256']}")
    print(f"CAPTURED_PREFIX_SHA256={report['captured_prefix_sha256']}")
    if first_difference is not None:
        print(f"FIRST_DIFFERENCE_BYTE={first_difference}")

    return 0 if exact else 1


if __name__ == "__main__":
    raise SystemExit(main())
