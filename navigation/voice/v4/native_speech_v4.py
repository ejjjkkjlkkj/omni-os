#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import math
import re
import struct
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

SAMPLE_RATE = 48_000
MAX_I16 = 32767
MIN_I16 = -32768
TAU = math.tau

@dataclass(frozen=True)
class VoiceProfile:
    name: str
    base_f0: float
    rate: float
    brightness: float
    breath: float
    formant_scale: float
    pitch_range: float
    spectral_tilt: float = 0.0
    jitter: float = 0.0
    shimmer: float = 0.0
    warmth: float = 1.0

@dataclass(frozen=True)
class Phoneme:
    symbol: str
    kind: str
    duration_ms: int
    formants: tuple[float, float, float, float]
    bandwidths: tuple[float, float, float, float]
    voiced: float = 1.0
    noise: float = 0.0

VOICES: dict[str, VoiceProfile] = {
    "clair": VoiceProfile("clair", 154.0, 1.00, 1.10, 0.035, 1.03, 0.18, -0.04, 0.003, 0.018, 0.96),
    "velours": VoiceProfile("velours", 124.0, 0.96, 0.92, 0.025, 0.98, 0.14, 0.08, 0.002, 0.014, 1.08),
    "screen": VoiceProfile("screen", 142.0, 1.18, 1.04, 0.020, 1.01, 0.10, -0.02, 0.001, 0.010, 1.00),
    "grave": VoiceProfile("grave", 102.0, 0.94, 0.86, 0.020, 0.95, 0.12, 0.10, 0.002, 0.012, 1.12),
    # Original adult female voices; not modeled on any real person.
    "femme": VoiceProfile("femme", 198.0, 1.00, 1.07, 0.043, 1.085, 0.23, -0.07, 0.004, 0.022, 1.01),
    "jeune_femme": VoiceProfile("jeune_femme", 226.0, 1.04, 1.13, 0.052, 1.125, 0.28, -0.11, 0.005, 0.026, 0.95),
}

def _p(s, kind, dur, f1=0, f2=0, f3=0, f4=0, b1=90, b2=120, b3=160, b4=220, voiced=1.0, noise=0.0):
    return Phoneme(s, kind, dur, (f1,f2,f3,f4), (b1,b2,b3,b4), voiced, noise)

PHONEMES: dict[str, Phoneme] = {
    "a": _p("a","v",105, 730,1090,2440,3300),
    "ɑ": _p("ɑ","v",110, 700,1050,2350,3200),
    "e": _p("e","v",100, 440,2000,2700,3500),
    "ɛ": _p("ɛ","v",105, 530,1840,2480,3400),
    "i": _p("i","v",95, 270,2290,3010,3700),
    "o": _p("o","v",105, 430,820,2500,3300),
    "ɔ": _p("ɔ","v",108, 570,840,2410,3250),
    "u": _p("u","v",100, 300,870,2240,3200),
    "y": _p("y","v",100, 310,1700,2400,3300),
    "ø": _p("ø","v",105, 420,1550,2480,3400),
    "œ": _p("œ","v",108, 520,1450,2400,3350),
    "ə": _p("ə","v",86, 500,1500,2500,3400),
    "ɑ̃": _p("ɑ̃","n",118, 640,1100,2200,3100, voiced=.92, noise=.05),
    "ɛ̃": _p("ɛ̃","n",116, 480,1750,2300,3200, voiced=.92, noise=.05),
    "ɔ̃": _p("ɔ̃","n",118, 520,900,2200,3100, voiced=.92, noise=.05),
    "œ̃": _p("œ̃","n",116, 520,1400,2250,3150, voiced=.92, noise=.05),
    "j": _p("j","g",62, 300,2200,3000,3700, voiced=.9),
    "w": _p("w","g",64, 300,900,2300,3200, voiced=.9),
    "ɥ": _p("ɥ","g",64, 320,1650,2400,3300, voiced=.9),
    "m": _p("m","n",72, 250,900,2050,3000, voiced=.9, noise=.04),
    "n": _p("n","n",68, 260,1000,2100,3050, voiced=.9, noise=.04),
    "ɲ": _p("ɲ","n",72, 300,1850,2600,3300, voiced=.9, noise=.04),
    "l": _p("l","l",70, 390,1500,2400,3300, voiced=.88),
    "ʁ": _p("ʁ","r",74, 500,1500,2400,3300, voiced=.52, noise=.36),
    "f": _p("f","f",78, 1000,2500,4800,6500, voiced=0, noise=1.0),
    "s": _p("s","f",78, 1800,4300,6500,7800, voiced=0, noise=1.0),
    "ʃ": _p("ʃ","f",82, 1500,3000,4700,6200, voiced=0, noise=1.0),
    "v": _p("v","f",78, 900,2200,4300,6200, voiced=.42, noise=.72),
    "z": _p("z","f",78, 1200,3000,5200,6800, voiced=.44, noise=.68),
    "ʒ": _p("ʒ","f",82, 1200,2600,4300,6000, voiced=.44, noise=.68),
    "p": _p("p","p",66, 900,2200,4500,6500, voiced=0, noise=.9),
    "t": _p("t","p",64, 1900,4000,6200,7900, voiced=0, noise=.95),
    "k": _p("k","p",70, 1400,2800,5000,7000, voiced=0, noise=.95),
    "b": _p("b","p",66, 500,1300,2300,3200, voiced=.56, noise=.52),
    "d": _p("d","p",64, 700,1600,2600,3500, voiced=.56, noise=.50),
    "g": _p("g","p",70, 600,1400,2400,3400, voiced=.56, noise=.50),
    "sil": _p("sil","s",55, voiced=0, noise=0),
}

DIGITS = {
    "0":"zéro","1":"un","2":"deux","3":"trois","4":"quatre","5":"cinq",
    "6":"six","7":"sept","8":"huit","9":"neuf"
}
LETTER_NAMES = {
    "a":"a","b":"bé","c":"cé","d":"dé","e":"e","f":"èfe","g":"gé","h":"ache","i":"i","j":"ji",
    "k":"ka","l":"èle","m":"ème","n":"ène","o":"o","p":"pé","q":"ku","r":"ère","s":"èsse",
    "t":"té","u":"u","v":"vé","w":"double vé","x":"ixe","y":"i grec","z":"zède"
}

def normalize_text(text: str) -> str:
    text = text.lower().replace("’", "'")
    text = re.sub(r"(?<=\d)[,.](?=\d)", " virgule ", text)
    def repl_num(m: re.Match[str]) -> str:
        token = m.group(0)
        if len(token) == 1:
            return DIGITS[token]
        return " ".join(DIGITS[d] for d in token)
    text = re.sub(r"\d+", repl_num, text)
    text = text.replace("uefi", "u e f i").replace("nvda", "n v d a")
    text = re.sub(r"([:;!?])", r" \1 ", text)
    text = re.sub(r"([.,])", r" \1 ", text)
    text = re.sub(r"[^a-zàâäéèêëîïôöùûüÿçœæ'\-.,:;!? ]+", " ", text)
    return re.sub(r"\s+", " ", text).strip()

def _word_to_phonemes(word: str) -> list[str]:
    if not word:
        return []
    lex = {
        "un":["œ̃"], "deux":["d","ø"], "trois":["t","ʁ","w","a"], "aide":["ɛ","d"],
        "oui":["w","i"], "non":["n","ɔ̃"], "erreur":["ɛ","ʁ","œ","ʁ"], "menu":["m","ə","n","y"],
        "continuer":["k","ɔ̃","t","i","n","y","e"], "récupération":["ʁ","e","k","y","p","e","ʁ","a","s","j","ɔ̃"],
        "recuperation":["ʁ","e","k","y","p","e","ʁ","a","s","j","ɔ̃"], "bonjour":["b","ɔ̃","ʒ","u","ʁ"],
        "voix":["v","w","a"], "système":["s","i","s","t","ɛ","m"], "systeme":["s","i","s","t","ɛ","m"],
        "lecteur":["l","ɛ","k","t","œ","ʁ"], "écran":["e","k","ʁ","ɑ̃"], "ecran":["e","k","ʁ","ɑ̃"],
        "windows":["w","i","n","d","o","z"], "bios":["b","j","o","s"], "audio":["o","d","j","o"],
        "usb":["y","ɛ","s","b","e"], "pci":["p","e","s","e","i"], "hda":["a","ʃ","d","e","a"],
    }
    if word in lex:
        return lex[word]
    if len(word) == 1 and word in LETTER_NAMES:
        return _word_to_phonemes(LETTER_NAMES[word]) if LETTER_NAMES[word] != word else [word]

    w = word
    out: list[str] = []
    i = 0
    while i < len(w):
        rest = w[i:]
        rules = [
            ("eaux",["o"]), ("eau",["o"]), ("ain",["ɛ̃"]), ("ein",["ɛ̃"]), ("aim",["ɛ̃"]),
            ("oin",["w","ɛ̃"]), ("ien",["j","ɛ̃"]), ("ill",["j"]), ("gn",["ɲ"]),
            ("ch",["ʃ"]), ("ph",["f"]), ("th",["t"]), ("qu",["k"]), ("gu",["g"]),
            ("ou",["u"]), ("oi",["w","a"]), ("ai",["ɛ"]), ("ei",["ɛ"]), ("au",["o"]),
            ("eu",["ø"]), ("oeu",["œ"]), ("œu",["œ"]), ("on",["ɔ̃"]), ("om",["ɔ̃"]),
            ("an",["ɑ̃"]), ("am",["ɑ̃"]), ("en",["ɑ̃"]), ("em",["ɑ̃"]),
            ("in",["ɛ̃"]), ("im",["ɛ̃"]), ("yn",["ɛ̃"]), ("ym",["ɛ̃"]),
            ("un",["œ̃"]), ("um",["œ̃"]),
        ]
        matched = False
        for g, ph in rules:
            if rest.startswith(g):
                out.extend(ph); i += len(g); matched = True; break
        if matched:
            continue
        ch = w[i]
        nxt = w[i+1] if i+1 < len(w) else ""
        prev = w[i-1] if i else ""
        if ch in "aàâä": out.append("a")
        elif ch in "eéèêë":
            if ch == "é": out.append("e")
            elif ch in "èêë": out.append("ɛ")
            elif i == len(w)-1: pass
            else: out.append("ə")
        elif ch in "iîïÿ": out.append("i")
        elif ch in "oôö": out.append("o" if ch != "o" else "ɔ")
        elif ch in "uùûü": out.append("y")
        elif ch == "y": out.append("j" if i and nxt else "i")
        elif ch == "c": out.append("s" if nxt in "eéiiy" else "k")
        elif ch == "ç": out.append("s")
        elif ch == "g": out.append("ʒ" if nxt in "eéiiy" else "g")
        elif ch == "j": out.append("ʒ")
        elif ch == "r": out.append("ʁ")
        elif ch == "h": pass
        elif ch == "x": out.extend(["k","s"])
        elif ch == "q": out.append("k")
        elif ch in "bd fklmnpstvz".replace(" ",""): out.append(ch)
        elif ch == "s":
            out.append("z" if prev in "aeiouyéèêàâùûô" and nxt in "aeiouyéèêàâùûô" else "s")
        elif ch == "'": pass
        elif ch == "-": pass
        i += 1
    if len(word) > 2 and out and word[-1] in "tdpsxzg" and not word.endswith(("ct","rt")):
        if out[-1] in {"t","d","p","s","z","g"}:
            out.pop()
    return out or ["ə"]

def text_to_phonemes(text: str) -> list[str]:
    norm = normalize_text(text)
    tokens = re.findall(r"[a-zàâäéèêëîïôöùûüÿçœæ'\-]+|[.,:;!?]", norm)
    out: list[str] = []
    for tok in tokens:
        if tok in {".", "!", "?"}:
            out += ["sil", "sil", "sil"]
        elif tok in {",", ":", ";"}:
            out += ["sil", "sil"]
        else:
            out += _word_to_phonemes(tok)
            out.append("sil")
    while out and out[-1] == "sil":
        out.pop()
    return out

class _Resonator:
    __slots__ = ("r","c","y1","y2")
    def __init__(self, freq: float, bw: float):
        self.configure(freq, bw)
        self.y1 = 0.0
        self.y2 = 0.0
    def configure(self, freq: float, bw: float) -> None:
        freq = max(60.0, min(freq, SAMPLE_RATE * 0.45))
        bw = max(40.0, bw)
        self.r = math.exp(-math.pi * bw / SAMPLE_RATE)
        self.c = 2.0 * self.r * math.cos(TAU * freq / SAMPLE_RATE)
    def step(self, x: float) -> float:
        y = (1.0 - self.r) * x + self.c * self.y1 - (self.r * self.r) * self.y2
        self.y2, self.y1 = self.y1, y
        return y

def _noise(seed: int) -> tuple[int, float]:
    seed ^= (seed << 13) & 0xFFFFFFFF
    seed ^= seed >> 17
    seed ^= (seed << 5) & 0xFFFFFFFF
    seed &= 0xFFFFFFFF
    return seed, ((seed & 0xFFFF) / 32767.5) - 1.0

def _softclip(x: float) -> float:
    return math.tanh(x * 1.30) / math.tanh(1.30)

def _phrase_pitch_multiplier(text: str, pos: float) -> float:
    pos = max(0.0, min(1.0, pos))
    contour = 1.035 - 0.075 * pos
    stripped = text.rstrip()
    if stripped.endswith("?"):
        contour += 0.18 * max(0.0, (pos - 0.72) / 0.28)
    elif stripped.endswith("!"):
        contour += 0.07 * max(0.0, (pos - 0.55) / 0.45)
    return contour

def _segment(symbol: str, voice: VoiceProfile, index: int, phrase_len: int, text: str) -> list[int]:
    spec = PHONEMES[symbol]
    n = max(1, int(SAMPLE_RATE * (spec.duration_ms / voice.rate) / 1000.0))
    if spec.kind == "s":
        return [0] * n
    pos = index / max(1, phrase_len - 1)
    f0_base = voice.base_f0 * _phrase_pitch_multiplier(text, pos) * (1.0 + voice.pitch_range * (0.06 - 0.09 * pos))
    rs = [_Resonator(f * voice.formant_scale * voice.brightness, bw) for f,bw in zip(spec.formants,spec.bandwidths)]
    seed = 0xC0FFEE11 ^ (index * 0x9E3779B1) ^ sum(ord(c) << (i % 8) for i,c in enumerate(symbol))
    out: list[int] = []
    phase = 0.0
    dc = 0.0
    prev = 0.0
    attack = max(1, int(SAMPLE_RATE * 0.0045))
    release = max(1, int(SAMPLE_RATE * 0.0065))
    for i in range(n):
        t = i / SAMPLE_RATE
        a = min(1.0, i / attack)
        r = min(1.0, (n - 1 - i) / release)
        env = max(0.0, min(a, r))
        vibr = 1.0 + (0.004 if voice.name == "screen" else 0.010) * math.sin(TAU * 4.8 * t + index * .37)
        seed, jit = _noise(seed)
        f0 = f0_base * vibr * (1.0 + voice.jitter * jit)
        phase += f0 / SAMPLE_RATE
        phase -= math.floor(phase)
        saw = 2.0 * phase - 1.0
        glottal = (1.0 - saw * saw) * (1.0 if phase < .58 else -0.34)
        glottal += 0.18 * math.sin(TAU * phase)
        seed, nz = _noise(seed)
        breath = voice.breath * nz
        amp_mod = 1.0 + voice.shimmer * (0.55 * math.sin(TAU * 3.1 * t + index * .19) + 0.45 * nz)
        if spec.kind == "p":
            burst_n = int(SAMPLE_RATE * 0.012)
            burst_env = math.exp(-i / max(1.0, burst_n * .33)) if i < burst_n * 3 else 0.0
            source = spec.voiced * glottal * 0.42 + spec.noise * nz * (0.88 * burst_env + 0.08)
        elif spec.kind in {"f","r"}:
            source = spec.voiced * glottal * 0.45 + spec.noise * nz * 0.72
        else:
            source = spec.voiced * glottal * 0.72 + spec.noise * nz * 0.26 + breath
        filt = 0.0
        gains = (1.0 * voice.warmth, 0.78, 0.48 * (1.0 - voice.spectral_tilt), 0.26 * (1.0 - voice.spectral_tilt))
        for g, res in zip(gains, rs):
            filt += g * res.step(source)
        x = (0.72 * filt + 0.16 * source) * env * amp_mod
        dc = 0.9975 * dc + 0.0025 * x
        x = x - dc + 0.08 * (x - prev)
        prev = x
        x = _softclip(x * 1.55) * 0.82
        out.append(max(MIN_I16, min(MAX_I16, int(round(x * MAX_I16)))))
    return out

def _crossfade(a: list[int], b: list[int], ms: float = 5.0) -> list[int]:
    if not a: return list(b)
    if not b: return list(a)
    n = min(len(a), len(b), max(1, int(SAMPLE_RATE * ms / 1000.0)))
    out = a[:-n]
    for i in range(n):
        x = (i + 1) / (n + 1)
        wa = math.cos(x * math.pi * .5)
        wb = math.sin(x * math.pi * .5)
        out.append(max(MIN_I16, min(MAX_I16, int(round(a[-n+i] * wa + b[i] * wb)))))
    out.extend(b[n:])
    return out

def synthesize(text: str, voice: str = "clair") -> list[int]:
    if voice not in VOICES:
        raise KeyError(f"unknown voice: {voice}")
    phs = text_to_phonemes(text)
    if not phs:
        return []
    v = VOICES[voice]
    out: list[int] = []
    for idx, ph in enumerate(phs):
        seg = _segment(ph, v, idx, len(phs), text)
        ms = 2.0 if PHONEMES[ph].kind in {"p","s"} else 6.0
        out = _crossfade(out, seg, ms)
    return _finalize(out)

def _finalize(samples: list[int]) -> list[int]:
    if not samples: return []
    mean = sum(samples) / len(samples)
    centered = [int(round(s - mean)) for s in samples]
    peak = max(1, max(abs(s) for s in centered))
    scale = min(1.0, (0.94 * MAX_I16) / peak)
    return [max(MIN_I16, min(MAX_I16, int(round(s * scale)))) for s in centered]

def pcm_s16le_mono(samples: Iterable[int]) -> bytes:
    return b"".join(struct.pack("<h", max(MIN_I16, min(MAX_I16, int(s)))) for s in samples)

def pcm_s16le_stereo(samples: Iterable[int]) -> bytes:
    out = bytearray()
    for s in samples:
        s = max(MIN_I16, min(MAX_I16, int(s)))
        out += struct.pack("<hh", s, s)
    return bytes(out)

def write_wav(path: str | Path, samples: list[int]) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(pcm_s16le_mono(samples))

def quality_metrics(samples: list[int]) -> dict[str, float]:
    if not samples:
        return {"peak":0.0, "rms":0.0, "dc":0.0, "clip_ratio":0.0, "duration_s":0.0}
    peak_i = max(abs(s) for s in samples)
    rms = math.sqrt(sum(float(s)*s for s in samples) / len(samples))
    dc = abs(sum(samples)/len(samples))
    clips = sum(1 for s in samples if abs(s) >= int(MAX_I16*.999))
    return {
        "peak": peak_i/MAX_I16,
        "rms": rms/MAX_I16,
        "dc": dc/MAX_I16,
        "clip_ratio": clips/len(samples),
        "duration_s": len(samples)/SAMPLE_RATE,
    }

def render_demo_set(out_dir: str | Path) -> dict[str, str]:
    out_dir = Path(out_dir)
    phrase = "Bonjour. Lecteur d'écran audio. UEFI, menu, continuer, récupération, erreur, non."
    result: dict[str,str] = {}
    for name in VOICES:
        samples = synthesize(phrase, name)
        p = out_dir / f"voicecore-v4-{name}.wav"
        write_wav(p, samples)
        result[name] = hashlib.sha256(p.read_bytes()).hexdigest()
    return result

if __name__ == "__main__":
    import argparse
    ap = argparse.ArgumentParser(description="VoiceCore v4 deterministic first-party French TTS")
    ap.add_argument("text", nargs="?", default="Bonjour. Synthèse vocale maison.")
    ap.add_argument("--voice", choices=sorted(VOICES), default="clair")
    ap.add_argument("--out", default="voicecore-v4.wav")
    args = ap.parse_args()
    samples = synthesize(args.text, args.voice)
    write_wav(args.out, samples)
    print(f"VOICECORE_V4=PASS voice={args.voice} samples={len(samples)} out={args.out}")
    print(quality_metrics(samples))
