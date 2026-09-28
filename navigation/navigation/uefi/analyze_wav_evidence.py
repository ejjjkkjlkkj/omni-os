#!/usr/bin/env python3
"""Validate that a QEMU WAV capture contains a real PCM signal.

This deliberately does not claim speech intelligibility. It proves only that
QEMU captured non-silent audio correlated with the UEFI runtime test.
"""

from __future__ import annotations

import json
import math
import sys
import wave
from pathlib import Path


def decode_samples(raw: bytes, width: int):
    if width == 1:
        for b in raw:
            yield b - 128
        return
    if width == 2:
        for i in range(0, len(raw) - 1, 2):
            yield int.from_bytes(raw[i:i + 2], "little", signed=True)
        return
    if width == 3:
        for i in range(0, len(raw) - 2, 3):
            b = raw[i:i + 3]
            sign = b"\xff" if b[2] & 0x80 else b"\x00"
            yield int.from_bytes(b + sign, "little", signed=True)
        return
    if width == 4:
        for i in range(0, len(raw) - 3, 4):
            yield int.from_bytes(raw[i:i + 4], "little", signed=True)
        return
    raise ValueError(f"unsupported sample width: {width}")


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print("usage: analyze_wav_evidence.py INPUT.wav [REPORT.json]", file=sys.stderr)
        return 2

    path = Path(sys.argv[1])
    report_path = Path(sys.argv[2]) if len(sys.argv) == 3 else None
    if not path.is_file() or path.stat().st_size <= 44:
        print("AUDIO_CAPTURE=FAIL")
        print("AUDIO_STREAM=FAIL")
        print("REASON=WAV_MISSING_OR_EMPTY")
        return 1

    try:
        with wave.open(str(path), "rb") as w:
            channels = w.getnchannels()
            width = w.getsampwidth()
            rate = w.getframerate()
            frames = w.getnframes()
            comptype = w.getcomptype()
            raw = w.readframes(frames)
    except (wave.Error, EOFError) as exc:
        print("AUDIO_CAPTURE=FAIL")
        print("AUDIO_STREAM=FAIL")
        print(f"REASON=WAV_INVALID:{exc}")
        return 1

    samples = list(decode_samples(raw, width))
    count = len(samples)
    peak = max((abs(x) for x in samples), default=0)
    rms = math.sqrt(sum(float(x) * float(x) for x in samples) / count) if count else 0.0
    active = sum(1 for x in samples if abs(x) >= 8)
    active_ratio = active / count if count else 0.0
    duration = frames / rate if rate else 0.0

    capture_ok = (
        comptype == "NONE"
        and channels >= 1
        and width in (1, 2, 3, 4)
        and rate > 0
        and frames > 0
        and duration > 0.05
    )
    # Deliberately permissive for quiet synthetic voices, but rejects silence
    # and WAV headers with no meaningful sample activity.
    signal_ok = capture_ok and peak >= 64 and rms >= 4.0 and active_ratio >= 0.0001

    report = {
        "path": str(path),
        "bytes": path.stat().st_size,
        "channels": channels,
        "sample_width_bytes": width,
        "sample_rate_hz": rate,
        "frames": frames,
        "duration_seconds": duration,
        "peak_abs": peak,
        "rms": rms,
        "active_sample_ratio": active_ratio,
        "compression": comptype,
        "audio_capture": "PASS" if capture_ok else "FAIL",
        "audio_stream": "PASS" if signal_ok else "FAIL",
        "speech_content": "NOT_ESTABLISHED",
        "intelligibility": "NOT_TESTED",
    }

    if report_path:
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(f"AUDIO_CAPTURE={'PASS' if capture_ok else 'FAIL'}")
    print(f"AUDIO_STREAM={'PASS' if signal_ok else 'FAIL'}")
    print("SPEECH_CONTENT=NOT_ESTABLISHED")
    print("INTELLIGIBILITY=NOT_TESTED")
    for key in (
        "channels", "sample_width_bytes", "sample_rate_hz", "frames",
        "duration_seconds", "peak_abs", "rms", "active_sample_ratio",
    ):
        print(f"WAV_{key.upper()}={report[key]}")

    return 0 if signal_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
