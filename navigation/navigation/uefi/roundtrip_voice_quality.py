#!/usr/bin/env python3
from __future__ import annotations

import json
import math
import struct
import sys
import wave
from pathlib import Path

MULAW_BIAS = 0x84
MULAW_CLIP = 32635


def linear_to_mulaw(sample: int) -> int:
    sample = max(-32768, min(32767, int(sample)))
    sign = 0x80 if sample < 0 else 0
    if sample < 0:
        sample = -sample
    sample = min(MULAW_CLIP, sample) + MULAW_BIAS
    exponent = 7
    mask = 0x4000
    while exponent > 0 and not (sample & mask):
        exponent -= 1
        mask >>= 1
    mantissa = (sample >> (exponent + 3)) & 0x0F
    return (~(sign | (exponent << 4) | mantissa)) & 0xFF


def mulaw_to_linear(code: int) -> int:
    u = (~int(code)) & 0xFF
    exponent = (u >> 4) & 0x07
    mantissa = u & 0x0F
    sample = ((mantissa << 3) + MULAW_BIAS) << exponent
    sample -= MULAW_BIAS
    return -sample if (u & 0x80) else sample


def upsample(samples: list[int], factor: int) -> list[int]:
    out: list[int] = []
    for i, a in enumerate(samples):
        b = samples[i + 1] if i + 1 < len(samples) else a
        for phase in range(factor):
            v = a + ((b - a) * phase) // factor
            out.append(max(-32768, min(32767, v)))
    return out


def main() -> int:
    if len(sys.argv) != 4:
        raise SystemExit("usage: roundtrip_voice_quality.py INPUT.wav OUTPUT.wav REPORT.json")

    src = Path(sys.argv[1])
    dst = Path(sys.argv[2])
    report_path = Path(sys.argv[3])

    with wave.open(str(src), "rb") as w:
        channels = w.getnchannels()
        width = w.getsampwidth()
        rate = w.getframerate()
        frames = w.getnframes()
        comptype = w.getcomptype()
        raw = w.readframes(frames)

    if channels != 1 or width != 2 or rate != 16000 or comptype != "NONE":
        raise SystemExit(
            f"expected PCM mono 16-bit 16000 Hz, got channels={channels} width={width} "
            f"rate={rate} compression={comptype}"
        )

    samples = list(struct.unpack("<" + "h" * (len(raw) // 2), raw))
    if not samples:
        raise SystemExit("empty input")

    encoded = bytes(linear_to_mulaw(x) for x in samples)
    decoded = [mulaw_to_linear(x) for x in encoded]

    ref48 = upsample(samples, 3)
    rt48 = upsample(decoded, 3)

    dst.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(dst), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(48000)
        stereo = bytearray()
        for s in rt48:
            stereo += struct.pack("<hh", s, s)
        w.writeframes(bytes(stereo))

    signal_power = sum(float(x) * float(x) for x in ref48) / len(ref48)
    error_power = sum(float(a - b) * float(a - b) for a, b in zip(ref48, rt48)) / len(ref48)
    snr_db = 99.0 if error_power <= 0.0 else 10.0 * math.log10(max(1e-12, signal_power / error_power))

    mean_a = sum(ref48) / len(ref48)
    mean_b = sum(rt48) / len(rt48)
    cov = sum((a - mean_a) * (b - mean_b) for a, b in zip(ref48, rt48))
    va = sum((a - mean_a) ** 2 for a in ref48)
    vb = sum((b - mean_b) ** 2 for b in rt48)
    corr = cov / math.sqrt(max(1.0, va * vb))

    peak = max(abs(x) for x in rt48)
    clip_ratio = sum(1 for x in rt48 if abs(x) >= 32760) / len(rt48)
    active_ratio = sum(1 for x in rt48 if abs(x) >= 256) / len(rt48)

    report = {
        "source": str(src),
        "source_rate_hz": rate,
        "firmware_output_rate_hz": 48000,
        "encoding": "g711-mulaw-u8",
        "firmware_interpolation": "linear-x3",
        "snr_db": snr_db,
        "correlation": corr,
        "peak_abs": peak,
        "clip_ratio": clip_ratio,
        "active_ratio": active_ratio,
    }

    passed = (
        snr_db >= 24.0
        and corr >= 0.985
        and peak >= 1000
        and clip_ratio <= 0.005
        and active_ratio >= 0.05
    )
    report["status"] = "PASS" if passed else "FAIL"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(f"VOICE_ROUNDTRIP_SNR_DB={snr_db:.2f}")
    print(f"VOICE_ROUNDTRIP_CORRELATION={corr:.6f}")
    print(f"VOICE_ROUNDTRIP_PEAK={peak}")
    print(f"VOICE_ROUNDTRIP_CLIP_RATIO={clip_ratio:.8f}")
    print(f"VOICE_ROUNDTRIP_ACTIVE_RATIO={active_ratio:.6f}")
    print("VOICE_ROUNDTRIP_QUALITY=" + ("PASS" if passed else "FAIL"))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
