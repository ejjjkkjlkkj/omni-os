#!/usr/bin/env python3
"""Regenerate the boot menu's speech clips with ST's neural voice (Kokoro-82M).

Replaces the Windows Speech API voice of gen-menu-speech.ps1 with a natural
one. Output format is what kernel/x86_64/src/hda.rs plays for speech: raw
24 kHz, signed 16-bit, mono PCM, one file per utterance in
kernel/x86_64/src/speech/. The .pcm files are committed; CI never re-renders.

Rendering is bit-reproducible: ONNX Runtime with a fixed thread count, and a
fresh session per clip seeded from the clip's own text (Kokoro's vocoder has
unseeded random operators whose generator lives in the session).

  <ST neural python> gen-menu-speech-st.py [--neural C:\\st\\neural] [--check]

--check re-renders every clip and fails if any committed file differs.
"""
import argparse
import hashlib
import pathlib
import sys

VOICE, LANG, SPEED, THREADS = "af_heart", "en-us", 1.0, 2
PHRASES = {
    "menu_title": "Accessible Windows, boot menu",
    "item_continue": "Continue and idle, menu item, 1 of 4",
    "item_sysinfo": "System information, menu item, 2 of 4",
    "item_reboot": "Reboot, menu item, 3 of 4",
    "item_poweroff": "Power off, menu item, 4 of 4",
}


def render(neural: pathlib.Path, text: str) -> bytes:
    import numpy as np
    import onnxruntime as rt
    from kokoro_onnx import Kokoro

    rt.set_seed(int.from_bytes(hashlib.sha256(f"{VOICE}|{LANG}|{SPEED}|{text}".encode()).digest()[:4], "little"))
    options = rt.SessionOptions()
    options.intra_op_num_threads = THREADS
    session = rt.InferenceSession(str(neural / "models/kokoro-v1.0.onnx"), sess_options=options,
                                  providers=["CPUExecutionProvider"])
    kokoro = Kokoro.from_session(session, str(neural / "models/voices-v1.0.bin"))
    x, rate = kokoro.create(text, voice=VOICE, speed=SPEED, lang=LANG)
    assert rate == 24000
    x = np.asarray(x, dtype=np.float64)
    x -= x.mean()
    loud = np.abs(x) > 0.01
    if loud.any():  # trim silences, keep 20 ms margins
        a, b = int(np.argmax(loud)), len(x) - int(np.argmax(loud[::-1]))
        x = x[max(0, a - 480):min(len(x), b + 480)]
    x *= 0.7 / max(float(np.abs(x).max()), 1e-6)
    fade = min(96, len(x) // 2)  # 4 ms: no click at start or end
    x[:fade] *= np.linspace(0, 1, fade)
    x[-fade:] *= np.linspace(1, 0, fade)
    return np.clip(np.round(x * 32767), -32767, 32767).astype("<i2").tobytes()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--neural", type=pathlib.Path, default=pathlib.Path(r"C:\st\neural"))
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    out_dir = pathlib.Path(__file__).resolve().parents[1] / "kernel/x86_64/src/speech"
    changed = []
    for name, text in PHRASES.items():
        pcm = render(args.neural, text)
        path = out_dir / f"{name}.pcm"
        if args.check:
            if not path.exists() or path.read_bytes() != pcm:
                changed.append(name)
        else:
            path.write_bytes(pcm)
        print(f"{name}: {len(pcm) / 48000:.2f} s  sha256={hashlib.sha256(pcm).hexdigest()[:16]}  {text!r}")
    if changed:
        print("MENU_SPEECH_DIFFERS:", ", ".join(changed))
        return 1
    print("MENU_SPEECH_REPRODUCIBLE" if args.check else "MENU_SPEECH_WRITTEN")
    return 0


if __name__ == "__main__":
    sys.exit(main())
