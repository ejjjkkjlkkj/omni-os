"""Build PHRASES.BIN: natural speech for the UEFI screen reader, pre-rendered by ST.

The reader normally spells labels letter by letter. For every HII prompt found in
a firmware image, this tool computes the exact text the reader will speak
(normalize_prompt + "<role> <label>" cut at 32 chars, see the C source) and
renders a natural phrase from the ORIGINAL label ("Checkbox: Above 4G Decoding")
with ST's neural backend (Kokoro-82M, 24 kHz). At runtime the reader hashes its
text, finds the clip and plays it; unknown texts keep the letter spelling.

Bank format (little endian):
  0  "QEVPHR01"      8  u32 count   12 u32 sample_rate (24000)
  16 u32 channels(1) 20 u32 bits(16) 24 u32 index_off  28 u32 data_off
  index: count x { u64 fnv1a64(text), u32 data offset, u32 byte length } sorted by hash
  data: signed 16-bit mono PCM clips

Run with ST's neural Python runtime:
  C:\\st\\work\\nextgen-venv\\Scripts\\python.exe build_phrase_bank.py M1603QAAS.308 PHRASES.BIN --st-neural C:\\st\\neural
"""
from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import pathlib
import struct
import uuid

PROMPT_OPS = {0x02, 0x03, 0x05, 0x06, 0x07, 0x08, 0x0C, 0x0D, 0x0F, 0x1A, 0x1B, 0x1C, 0x23}
ROLE = {0x02: "subtitle", 0x03: "text", 0x05: "choice", 0x06: "checkbox", 0x07: "number", 0x08: "password",
        0x0C: "button", 0x0D: "reset", 0x0F: "reference", 0x1A: "date", 0x1B: "time", 0x1C: "edit",
        0x23: "ordered list"}
FOLD = {**{c: "a" for c in (0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5)},
        0xC7: "c", 0xE7: "c",
        **{c: "e" for c in (0xC8, 0xC9, 0xCA, 0xCB, 0xE8, 0xE9, 0xEA, 0xEB)},
        **{c: "i" for c in (0xCC, 0xCD, 0xCE, 0xCF, 0xEC, 0xED, 0xEE, 0xEF)},
        **{c: "o" for c in (0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6)},
        **{c: "u" for c in (0xD9, 0xDA, 0xDB, 0xDC, 0xF9, 0xFA, 0xFB, 0xFC)},
        0x178: "y", 0xFF: "y"}
LZMA_GUID = uuid.UUID("EE4E5898-3914-4259-9D6E-DC7BD79403CF").bytes_le
RATE = 24000


def fnv1a64(text: str) -> int:
    h = 0xCBF29CE484222325
    for b in text.encode("ascii"):
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def normalize_prompt(text: str) -> str:
    """Port of fold_prompt_char()/normalize_prompt()."""
    out, pending = [], False
    for ch in text[:127]:
        if len(out) >= 32:
            break
        o = ord(ch)
        c = chr(o + 32) if 0x41 <= o <= 0x5A else FOLD.get(o, ch)
        if len(c) == 1 and "a" <= c <= "z":
            if pending and out and len(out) < 32:
                out.append(" ")
            if len(out) < 32:
                out.append(c)
            pending = False
        elif out:
            pending = True
    return "".join(out)


def speech_text(opcode: int, label: str) -> str:
    """Port of nav_prompt_load(): role, space, label, cut at 32."""
    return (ROLE.get(opcode, "control") + (" " + label if label else ""))[:32]


def sections(image: bytes) -> list[bytes]:
    out = [image]

    def walk(buf: bytes, depth: int) -> None:
        pos = 0
        while (i := buf.find(LZMA_GUID, pos)) >= 0:
            pos = i + 16
            if i < 4 or buf[i - 1] != 0x02:
                continue
            size = int.from_bytes(buf[i - 4:i - 1], "little")
            offset = int.from_bytes(buf[i + 16:i + 18], "little")
            try:
                data = lzma.LZMADecompressor(format=lzma.FORMAT_ALONE).decompress(buf[i - 4 + offset:i - 4 + size])
            except lzma.LZMAError:
                continue
            out.append(data)
            if depth < 3:
                walk(data, depth + 1)

    walk(image, 0)
    return out


def parse_strings(pkg: bytes) -> dict[int, str]:
    p = struct.unpack_from("<I", pkg, 8)[0]
    sid, strings = 1, {}

    def ucs2(at: int) -> tuple[str, int]:
        end = at
        while pkg[end:end + 2] != b"\0\0":
            end += 2
        return pkg[at:end].decode("utf-16-le", "replace"), end + 2

    while p < len(pkg):
        t = pkg[p]
        if t == 0x14:
            strings[sid], p = ucs2(p + 1); sid += 1
        elif t == 0x15:
            n = struct.unpack_from("<H", pkg, p + 1)[0]; p += 3
            for _ in range(n):
                strings[sid], p = ucs2(p); sid += 1
        elif t == 0x16:
            strings[sid], p = ucs2(p + 2); sid += 1
        elif t == 0x17:
            n = struct.unpack_from("<H", pkg, p + 2)[0]; p += 4
            for _ in range(n):
                strings[sid], p = ucs2(p); sid += 1
        elif t == 0x20:
            strings[sid] = strings.get(struct.unpack_from("<H", pkg, p + 1)[0], ""); sid += 1; p += 3
        elif t == 0x21:
            sid += pkg[p + 1]; p += 2
        elif t == 0x22:
            sid += struct.unpack_from("<H", pkg, p + 1)[0]; p += 3
        elif t == 0x30:
            p += pkg[p + 2]
        elif t == 0x31:
            p += struct.unpack_from("<H", pkg, p + 2)[0]
        elif t == 0x32:
            p += struct.unpack_from("<I", pkg, p + 2)[0]
        else:  # SIBT_END, SCSU or unknown: stop
            break
    return strings


def phrases(image: bytes) -> dict[str, str]:
    """spoken text (hash key) -> text to render from the original label."""
    found: dict[str, str] = {}
    for blob in sections(image):
        forms, strings = [], []
        for i in range(len(blob) - 52):
            t = blob[i + 3]
            if t not in (0x02, 0x04):
                continue
            length = int.from_bytes(blob[i:i + 3], "little")
            if not 16 < length < 0x200000 or i + length > len(blob):
                continue
            pkg = blob[i:i + length]
            if t == 0x02 and pkg[4] == 0x0E and pkg[5] & 0x80:
                forms.append((i, pkg))
            elif t == 0x04 and 46 <= struct.unpack_from("<I", pkg, 4)[0] < 200 and pkg[46:51] in (b"en-US", b"en-us"):
                strings.append((i, parse_strings(pkg)))
        if not strings:
            continue
        for off, pkg in forms:
            table = min(strings, key=lambda s: abs(s[0] - off))[1]
            q = 4
            while q + 2 <= len(pkg):
                op, oplen = pkg[q], pkg[q + 1] & 0x7F
                if oplen < 2 or q + oplen > len(pkg):
                    break
                if op in PROMPT_OPS and oplen >= 4:
                    raw = table.get(struct.unpack_from("<H", pkg, q + 2)[0], "")
                    label = normalize_prompt(raw)
                    if label:
                        key = speech_text(op, label)
                        clean = " ".join(raw.split())
                        found.setdefault(key, f"{ROLE.get(op, 'control')}: {clean}.")
                q += oplen
    return found


def render_all(items: dict[str, str], neural: pathlib.Path) -> dict[str, bytes]:
    import numpy as np
    import onnxruntime as rt
    from kokoro_onnx import Kokoro
    o = rt.SessionOptions()
    o.intra_op_num_threads = 8
    k = Kokoro.from_session(rt.InferenceSession(str(neural / "models/kokoro-v1.0.onnx"), sess_options=o,
                                                providers=["CPUExecutionProvider"]), str(neural / "models/voices-v1.0.bin"))
    clips = {}
    for n, (key, text) in enumerate(sorted(items.items())):
        x, rate = k.create(text, voice="af_heart", speed=1.05, lang="en-us")
        assert rate == RATE
        x = np.asarray(x, dtype=np.float64)
        x -= x.mean()
        env = np.abs(x) > 0.01
        if env.any():  # trim leading/trailing silence, keep 20 ms margins
            a, b = np.argmax(env), len(x) - np.argmax(env[::-1])
            x = x[max(0, a - 480):min(len(x), b + 480)]
        x *= 0.70 / max(np.abs(x).max(), 1e-6)
        fade = min(120, len(x) // 2)
        x[:fade] *= np.linspace(0, 1, fade)
        x[-fade:] *= np.linspace(1, 0, fade)
        clips[key] = np.clip(np.round(x * 32767), -32767, 32767).astype("<i2").tobytes()
        if n % 100 == 0:
            print(f"{n}/{len(items)} {key!r}", flush=True)
    return clips


def write_bank(clips: dict[str, bytes], out: pathlib.Path) -> dict:
    entries = sorted(((fnv1a64(k), k) for k in clips), key=lambda e: e[0])
    hashes = [h for h, _ in entries]
    if len(set(hashes)) != len(hashes):
        raise ValueError("FNV-1a collision between spoken texts")
    index_off, data_off = 32, 32 + 16 * len(entries)
    index, data = bytearray(), bytearray()
    for h, key in entries:
        index += struct.pack("<QII", h, len(data), len(clips[key]))
        data += clips[key]
    out.write_bytes(b"QEVPHR01" + struct.pack("<IIIIII", len(entries), RATE, 1, 16, index_off, data_off) + index + data)
    return {"count": len(entries), "bytes": out.stat().st_size, "sha256": hashlib.sha256(out.read_bytes()).hexdigest(),
            "seconds": len(data) / 2 / RATE}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("image", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--st-neural", type=pathlib.Path, required=True)
    ap.add_argument("--manifest", type=pathlib.Path)
    a = ap.parse_args()
    image = a.image.read_bytes()
    items = phrases(image)
    print(f"{len(items)} phrases from {a.image.name}", flush=True)
    info = write_bank(render_all(items, a.st_neural), a.out)
    info.update(image=a.image.name, image_sha256=hashlib.sha256(image).hexdigest(), voice="af_heart",
                engine="ST neural (Kokoro-82M v1.0)", phrases=dict(sorted(items.items())))
    if a.manifest:
        a.manifest.write_text(json.dumps(info, indent=1), encoding="utf-8")
    print(json.dumps({k: v for k, v in info.items() if k != "phrases"}, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
