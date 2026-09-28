#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import math
import re
import struct
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Iterator

SAMPLE_RATE = 48_000
MAX_I16 = 32767
MIN_I16 = -32768
TAU = math.tau
ENGINE_NAME = "VoiceCore v4"
ENGINE_ABI = 2
DEFAULT_CHUNK_FRAMES = 960  # 20 ms at 48 kHz

@dataclass(frozen=True)
class VoiceProfile:
    name: str
    base_f0: float
    rate: float
    formant_scale: float
    brightness: float
    breath: float
    pitch_range: float
    energy: float
    spectral_tilt: float = 0.0
    jitter: float = 0.0
    shimmer: float = 0.0

@dataclass(frozen=True)
class PhoneSpec:
    symbol: str
    kind: str
    duration_ms: int
    formants: tuple[float, float, float, float]
    bandwidths: tuple[float, float, float, float]
    voiced: float
    noise: float

@dataclass(frozen=True)
class PhoneEvent:
    symbol: str
    duration_scale: float
    pitch_scale: float
    energy_scale: float

VOICES: dict[str, VoiceProfile] = {
    "screen": VoiceProfile("screen", 146.0, 1.20, 1.01, 1.03, 0.015, 0.08, 0.98),
    "clair": VoiceProfile("clair", 158.0, 1.00, 1.03, 1.08, 0.028, 0.16, 1.00),
    "velours": VoiceProfile("velours", 126.0, 0.94, 0.98, 0.93, 0.020, 0.12, 0.94),
    "grave": VoiceProfile("grave", 104.0, 0.92, 0.95, 0.88, 0.018, 0.10, 0.96),
    "rapide": VoiceProfile("rapide", 150.0, 1.34, 1.01, 1.02, 0.014, 0.07, 0.95),
    "compact": VoiceProfile("compact", 138.0, 1.12, 1.00, 1.00, 0.012, 0.06, 0.92),
    # Original adult female voices. They are not modeled on any real person.
    "femme": VoiceProfile("femme", 196.0, 0.99, 1.08, 1.06, 0.040, 0.22, 0.98, -0.07, 0.0035, 0.018),
    "jeune_femme": VoiceProfile("jeune_femme", 224.0, 1.04, 1.12, 1.12, 0.050, 0.28, 0.96, -0.11, 0.0045, 0.022),
}

def _p(symbol: str, kind: str, duration_ms: int, formants=(0.0,0.0,0.0,0.0),
       bandwidths=(90.0,120.0,170.0,230.0), voiced=1.0, noise=0.0) -> PhoneSpec:
    return PhoneSpec(symbol, kind, duration_ms, tuple(float(x) for x in formants),
                     tuple(float(x) for x in bandwidths), float(voiced), float(noise))

PHONES: dict[str, PhoneSpec] = {
    "a": _p("a","v",104,(730,1090,2440,3300)),
    "ɑ": _p("ɑ","v",108,(700,1050,2350,3200)),
    "e": _p("e","v",98,(440,2000,2700,3500)),
    "ɛ": _p("ɛ","v",104,(530,1840,2480,3400)),
    "i": _p("i","v",94,(270,2290,3010,3700)),
    "o": _p("o","v",103,(430,820,2500,3300)),
    "ɔ": _p("ɔ","v",106,(570,840,2410,3250)),
    "u": _p("u","v",98,(300,870,2240,3200)),
    "y": _p("y","v",98,(310,1700,2400,3300)),
    "ø": _p("ø","v",102,(420,1550,2480,3400)),
    "œ": _p("œ","v",106,(520,1450,2400,3350)),
    "ə": _p("ə","v",82,(500,1500,2500,3400)),
    "ɑ̃": _p("ɑ̃","n",115,(640,1100,2200,3100),voiced=.92,noise=.05),
    "ɛ̃": _p("ɛ̃","n",113,(480,1750,2300,3200),voiced=.92,noise=.05),
    "ɔ̃": _p("ɔ̃","n",115,(520,900,2200,3100),voiced=.92,noise=.05),
    "œ̃": _p("œ̃","n",113,(520,1400,2250,3150),voiced=.92,noise=.05),
    "j": _p("j","g",58,(300,2200,3000,3700),voiced=.90),
    "w": _p("w","g",60,(300,900,2300,3200),voiced=.90),
    "ɥ": _p("ɥ","g",60,(320,1650,2400,3300),voiced=.90),
    "m": _p("m","n",70,(250,900,2050,3000),voiced=.90,noise=.04),
    "n": _p("n","n",66,(260,1000,2100,3050),voiced=.90,noise=.04),
    "ɲ": _p("ɲ","n",70,(300,1850,2600,3300),voiced=.90,noise=.04),
    "ŋ": _p("ŋ","n",70,(350,1200,2200,3100),voiced=.88,noise=.05),
    "l": _p("l","l",68,(390,1500,2400,3300),voiced=.88),
    "ʁ": _p("ʁ","r",72,(500,1500,2400,3300),voiced=.52,noise=.36),
    "f": _p("f","f",76,(1000,2500,4800,6500),voiced=0.0,noise=1.0),
    "s": _p("s","f",76,(1800,4300,6500,7800),voiced=0.0,noise=1.0),
    "ʃ": _p("ʃ","f",80,(1500,3000,4700,6200),voiced=0.0,noise=1.0),
    "v": _p("v","f",76,(900,2200,4300,6200),voiced=.42,noise=.72),
    "z": _p("z","f",76,(1200,3000,5200,6800),voiced=.44,noise=.68),
    "ʒ": _p("ʒ","f",80,(1200,2600,4300,6000),voiced=.44,noise=.68),
    "p": _p("p","p",64,(900,2200,4500,6500),voiced=0.0,noise=.90),
    "t": _p("t","p",62,(1900,4000,6200,7900),voiced=0.0,noise=.95),
    "k": _p("k","p",68,(1400,2800,5000,7000),voiced=0.0,noise=.95),
    "b": _p("b","p",64,(500,1300,2300,3200),voiced=.56,noise=.52),
    "d": _p("d","p",62,(700,1600,2600,3500),voiced=.56,noise=.50),
    "g": _p("g","p",68,(600,1400,2400,3400),voiced=.56,noise=.50),
    "sil": _p("sil","s",50,voiced=0.0,noise=0.0),
}

LETTER_NAMES = {
    "a":"a","b":"bé","c":"cé","d":"dé","e":"e","f":"èfe","g":"gé","h":"ache","i":"i","j":"ji",
    "k":"ka","l":"èle","m":"ème","n":"ène","o":"o","p":"pé","q":"ku","r":"ère","s":"èsse","t":"té",
    "u":"u","v":"vé","w":"double vé","x":"ixe","y":"i grec","z":"zède"
}

TECH_WORDS = {
    "uefi":"u e f i", "bios":"b i o s", "hda":"h d a", "usb":"u s b", "pci":"p c i",
    "nvda":"n v d a", "tpm":"t p m", "cpu":"c p u", "gpu":"g p u", "efi":"e f i",
}

LEXICON: dict[str, list[str]] = {
    "un":["œ̃"], "deux":["d","ø"], "trois":["t","ʁ","w","a"], "quatre":["k","a","t","ʁ"],
    "cinq":["s","ɛ̃","k"], "six":["s","i","s"], "sept":["s","ɛ","t"], "huit":["ɥ","i","t"],
    "neuf":["n","œ","f"], "zéro":["z","e","ʁ","o"], "zero":["z","e","ʁ","o"],
    "dix":["d","i","s"], "onze":["ɔ̃","z"], "douze":["d","u","z"], "treize":["t","ʁ","ɛ","z"],
    "quatorze":["k","a","t","ɔ","ʁ","z"], "quinze":["k","ɛ̃","z"], "seize":["s","ɛ","z"],
    "vingt":["v","ɛ̃"], "trente":["t","ʁ","ɑ̃","t"], "quarante":["k","a","ʁ","ɑ̃","t"],
    "cinquante":["s","ɛ̃","k","ɑ̃","t"], "soixante":["s","w","a","s","ɑ̃","t"],
    "cent":["s","ɑ̃"], "mille":["m","i","l"],
    "oui":["w","i"], "non":["n","ɔ̃"], "erreur":["ɛ","ʁ","œ","ʁ"], "menu":["m","ə","n","y"],
    "continuer":["k","ɔ̃","t","i","n","y","e"], "récupération":["ʁ","e","k","y","p","e","ʁ","a","s","j","ɔ̃"],
    "recuperation":["ʁ","e","k","y","p","e","ʁ","a","s","j","ɔ̃"], "bonjour":["b","ɔ̃","ʒ","u","ʁ"],
    "voix":["v","w","a"], "système":["s","i","s","t","ɛ","m"], "systeme":["s","i","s","t","ɛ","m"],
    "lecteur":["l","ɛ","k","t","œ","ʁ"], "écran":["e","k","ʁ","ɑ̃"], "ecran":["e","k","ʁ","ɑ̃"],
    "audio":["o","d","j","o"], "sécurité":["s","e","k","y","ʁ","i","t","e"], "securite":["s","e","k","y","ʁ","i","t","e"],
    "activé":["a","k","t","i","v","e"], "active":["a","k","t","i","v","e"],
    "désactivé":["d","e","z","a","k","t","i","v","e"], "desactive":["d","e","z","a","k","t","i","v","e"],
    "démarrage":["d","e","m","a","ʁ","a","ʒ"], "demarrage":["d","e","m","a","ʁ","a","ʒ"],
}

_SMALL = {
    0:"zéro",1:"un",2:"deux",3:"trois",4:"quatre",5:"cinq",6:"six",7:"sept",8:"huit",9:"neuf",
    10:"dix",11:"onze",12:"douze",13:"treize",14:"quatorze",15:"quinze",16:"seize",
}

def integer_to_words(n: int) -> str:
    if n < 0:
        return "moins " + integer_to_words(-n)
    if n in _SMALL:
        return _SMALL[n]
    if n < 20:
        return "dix " + _SMALL[n - 10]
    if n < 70:
        tens = {20:"vingt",30:"trente",40:"quarante",50:"cinquante",60:"soixante"}
        t = (n // 10) * 10
        r = n % 10
        return tens[t] if r == 0 else tens[t] + (" et " if r == 1 else " ") + _SMALL[r]
    if n < 80:
        return "soixante " + integer_to_words(n - 60)
    if n < 100:
        return "quatre vingt" if n == 80 else "quatre vingt " + integer_to_words(n - 80)
    if n < 1000:
        h, r = divmod(n, 100)
        head = "cent" if h == 1 else _SMALL[h] + " cent"
        return head if r == 0 else head + " " + integer_to_words(r)
    if n < 10000:
        th, r = divmod(n, 1000)
        head = "mille" if th == 1 else integer_to_words(th) + " mille"
        return head if r == 0 else head + " " + integer_to_words(r)
    return " ".join(_SMALL[int(d)] for d in str(n))

def normalize_text(text: str) -> str:
    text = text.replace("’", "'")
    for key, value in TECH_WORDS.items():
        text = re.sub(rf"\b{re.escape(key)}\b", value, text, flags=re.IGNORECASE)

    def decimal_repl(m: re.Match[str]) -> str:
        left, right = m.group(1), m.group(2)
        return f"{integer_to_words(int(left))} virgule " + " ".join(_SMALL[int(d)] for d in right)

    text = re.sub(r"\b(\d+)[,.](\d+)\b", decimal_repl, text)
    text = re.sub(r"\b\d+\b", lambda m: integer_to_words(int(m.group(0))), text)
    text = text.lower()
    text = re.sub(r"([.,:;!?])", r" \1 ", text)
    text = re.sub(r"[^a-zàâäéèêëîïôöùûüÿçœæ'\-.,:;!? ]+", " ", text)
    return re.sub(r"\s+", " ", text).strip()

def _word_to_phones(word: str) -> list[str]:
    if not word:
        return []
    if word in LEXICON:
        return list(LEXICON[word])
    if len(word) == 1 and word in LETTER_NAMES:
        spoken = LETTER_NAMES[word]
        if spoken == word:
            return [word]
        return _word_to_phones(spoken)

    rules = [
        ("eaux",["o"]),("eau",["o"]),("ain",["ɛ̃"]),("ein",["ɛ̃"]),("aim",["ɛ̃"]),
        ("oin",["w","ɛ̃"]),("ien",["j","ɛ̃"]),("ill",["j"]),("gn",["ɲ"]),("ch",["ʃ"]),
        ("ph",["f"]),("th",["t"]),("qu",["k"]),("gu",["g"]),("ou",["u"]),("oi",["w","a"]),
        ("ai",["ɛ"]),("ei",["ɛ"]),("au",["o"]),("oeu",["œ"]),("œu",["œ"]),("eu",["ø"]),
        ("on",["ɔ̃"]),("om",["ɔ̃"]),("an",["ɑ̃"]),("am",["ɑ̃"]),("en",["ɑ̃"]),("em",["ɑ̃"]),
        ("in",["ɛ̃"]),("im",["ɛ̃"]),("yn",["ɛ̃"]),("ym",["ɛ̃"]),("un",["œ̃"]),("um",["œ̃"]),
        ("ng",["ŋ"]),
    ]
    out: list[str] = []
    i = 0
    vowels = "aeiouyéèêàâùûôœ"
    while i < len(word):
        rest = word[i:]
        matched = False
        for grapheme, phones in rules:
            if rest.startswith(grapheme):
                out.extend(phones)
                i += len(grapheme)
                matched = True
                break
        if matched:
            continue

        ch = word[i]
        nxt = word[i + 1] if i + 1 < len(word) else ""
        prev = word[i - 1] if i else ""
        if ch in "aàâä": out.append("a")
        elif ch in "eéèêë":
            if ch == "é": out.append("e")
            elif ch in "èêë": out.append("ɛ")
            elif i != len(word) - 1: out.append("ə")
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
        elif ch in "bdfklmnpstvz": out.append(ch)
        elif ch == "s": out.append("z" if prev in vowels and nxt in vowels else "s")
        i += 1

    if len(word) > 2 and out and word[-1] in "tdpsxzg" and not word.endswith(("ct","rt")):
        if out[-1] in {"t","d","p","s","z","g"}:
            out.pop()
    return out or ["ə"]

def text_to_events(text: str) -> list[PhoneEvent]:
    tokens = re.findall(r"[a-zàâäéèêëîïôöùûüÿçœæ'\-]+|[.,:;!?]", normalize_text(text))
    events: list[PhoneEvent] = []
    word_index = 0
    words_total = max(1, sum(tok not in ".,:;!?" for tok in tokens))
    for tok in tokens:
        if tok in {".","!","?"}:
            # Apply sentence-final prosody to the most recent spoken phones.
            recent = [idx for idx, e in enumerate(events) if e.symbol != "sil"][-8:]
            if recent and tok in {"?","!"}:
                for rank, idx in enumerate(recent, start=1):
                    e = events[idx]
                    ratio = rank / len(recent)
                    if tok == "?":
                        events[idx] = PhoneEvent(
                            e.symbol, e.duration_scale,
                            e.pitch_scale * (1.0 + 0.16 * ratio),
                            e.energy_scale,
                        )
                    else:
                        events[idx] = PhoneEvent(
                            e.symbol, e.duration_scale,
                            e.pitch_scale * (1.0 + 0.06 * ratio),
                            e.energy_scale * (1.0 + 0.08 * ratio),
                        )
            count = 3 if tok == "." else 4
            for _ in range(count):
                events.append(PhoneEvent("sil", 1.0, 1.0, 0.0))
            continue
        if tok in {",",":",";"}:
            for _ in range(2):
                events.append(PhoneEvent("sil", 1.0, 1.0, 0.0))
            continue

        phones = _word_to_phones(tok)
        sentence_pos = word_index / max(1, words_total - 1)
        base_pitch = 1.04 - 0.08 * sentence_pos
        for pi, ph in enumerate(phones):
            edge = pi / max(1, len(phones) - 1)
            local_pitch = base_pitch * (1.0 + 0.018 * math.sin(math.pi * edge))
            duration = 1.0
            if pi == len(phones) - 1:
                duration = 1.05
            events.append(PhoneEvent(ph, duration, local_pitch, 1.0))
        events.append(PhoneEvent("sil", 0.55, 1.0, 0.0))
        word_index += 1

    while events and events[-1].symbol == "sil":
        events.pop()
    return events

class Resonator:
    __slots__ = ("r","c","y1","y2")
    def __init__(self, freq: float, bandwidth: float):
        self.y1 = 0.0
        self.y2 = 0.0
        self.configure(freq, bandwidth)
    def configure(self, freq: float, bandwidth: float) -> None:
        freq = max(60.0, min(freq, SAMPLE_RATE * 0.45))
        bandwidth = max(35.0, bandwidth)
        self.r = math.exp(-math.pi * bandwidth / SAMPLE_RATE)
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

def _neighbour_formants(events: list[PhoneEvent], index: int) -> tuple[tuple[float,...], tuple[float,...]]:
    spec = PHONES[events[index].symbol]
    prev = spec.formants
    nxt = spec.formants
    for j in range(index - 1, -1, -1):
        s = PHONES[events[j].symbol]
        if s.kind != "s":
            prev = s.formants
            break
    for j in range(index + 1, len(events)):
        s = PHONES[events[j].symbol]
        if s.kind != "s":
            nxt = s.formants
            break
    return prev, nxt

def _segment(events: list[PhoneEvent], index: int, voice: VoiceProfile) -> list[int]:
    event = events[index]
    spec = PHONES[event.symbol]
    n = max(1, int(SAMPLE_RATE * (spec.duration_ms * event.duration_scale / voice.rate) / 1000.0))
    if spec.kind == "s":
        return [0] * n

    prev_f, next_f = _neighbour_formants(events, index)
    resonators = [Resonator(f * voice.formant_scale, bw) for f, bw in zip(spec.formants, spec.bandwidths)]
    seed = 0x56433411 ^ (index * 0x9E3779B1) ^ sum(ord(c) << (i % 8) for i, c in enumerate(spec.symbol))
    phase = 0.0
    dc = 0.0
    prev_x = 0.0
    attack = max(1, int(SAMPLE_RATE * 0.004))
    release = max(1, int(SAMPLE_RATE * 0.006))
    out: list[int] = []

    for i in range(n):
        pos = i / max(1, n - 1)
        env = max(0.0, min(1.0, i / attack, (n - 1 - i) / release))
        # Coarticulation window: first/last 22% interpolate toward neighbours.
        if pos < 0.22:
            blend = (0.22 - pos) / 0.22 * 0.35
            targets = tuple(spec.formants[k] * (1.0 - blend) + prev_f[k] * blend for k in range(4))
        elif pos > 0.78:
            blend = (pos - 0.78) / 0.22 * 0.35
            targets = tuple(spec.formants[k] * (1.0 - blend) + next_f[k] * blend for k in range(4))
        else:
            targets = spec.formants

        if i % 24 == 0:
            for r, freq, bw in zip(resonators, targets, spec.bandwidths):
                r.configure(freq * voice.formant_scale * voice.brightness, bw)

        phrase_pos = index / max(1, len(events) - 1)
        f0 = voice.base_f0 * event.pitch_scale * (1.0 + voice.pitch_range * (0.08 - 0.12 * phrase_pos))
        vibrato = 1.0 + 0.006 * math.sin(TAU * 4.7 * (i / SAMPLE_RATE) + index * 0.31)
        seed, jitter_noise = _noise(seed)
        f0 *= 1.0 + voice.jitter * jitter_noise
        phase = (phase + f0 * vibrato / SAMPLE_RATE) % 1.0
        # Smooth glottal source with harmonics; deterministic and cheap enough for offline generation.
        glottal = (
            0.64 * math.sin(TAU * phase) +
            0.22 * math.sin(2.0 * TAU * phase) +
            0.09 * math.sin(3.0 * TAU * phase)
        )
        seed, nz = _noise(seed)

        if spec.kind == "p":
            burst_len = max(1, int(SAMPLE_RATE * 0.012))
            burst = math.exp(-i / max(1.0, burst_len * 0.42)) if i < burst_len * 4 else 0.0
            source = spec.voiced * glottal * 0.40 + spec.noise * nz * (0.88 * burst + 0.06)
        elif spec.kind in {"f","r"}:
            source = spec.voiced * glottal * 0.44 + spec.noise * nz * 0.70
        else:
            source = spec.voiced * glottal * 0.72 + spec.noise * nz * 0.24 + voice.breath * nz

        gains = (
            1.0,
            0.78,
            0.48 * (1.0 - voice.spectral_tilt),
            0.25 * (1.0 - voice.spectral_tilt),
        )
        filtered = sum(g * r.step(source) for g, r in zip(gains, resonators))
        shimmer = 1.0 + voice.shimmer * math.sin(TAU * 3.2 * (i / SAMPLE_RATE) + index * 0.19)
        x = (0.73 * filtered + 0.15 * source) * env * event.energy_scale * voice.energy * shimmer
        dc = 0.9975 * dc + 0.0025 * x
        x = x - dc + 0.07 * (x - prev_x)
        prev_x = x
        x = math.tanh(x * 1.48) * 0.80
        sample = int(round(x * MAX_I16))
        out.append(max(MIN_I16, min(MAX_I16, sample)))
    return out

def _crossfade_samples(a_tail: list[int], b_head: list[int], count: int) -> list[int]:
    n = min(len(a_tail), len(b_head), max(0, count))
    out: list[int] = []
    for i in range(n):
        x = (i + 1) / (n + 1)
        wa = math.cos(x * math.pi * 0.5)
        wb = math.sin(x * math.pi * 0.5)
        v = int(round(a_tail[-n + i] * wa + b_head[i] * wb))
        out.append(max(MIN_I16, min(MAX_I16, v)))
    return out

def _fade_frames(kind: str) -> int:
    ms = 1.5 if kind in {"p","s"} else 4.0
    return max(1, int(SAMPLE_RATE * ms / 1000.0))

def synthesize_stream(text: str, voice: str = "screen",
                      chunk_frames: int = DEFAULT_CHUNK_FRAMES) -> Iterator[list[int]]:
    if voice not in VOICES:
        raise KeyError(f"unknown voice: {voice}")
    if chunk_frames <= 0:
        raise ValueError("chunk_frames must be positive")

    events = text_to_events(text)
    if not events:
        return

    profile = VOICES[voice]
    ready: list[int] = []
    pending: list[int] = []
    hp_prev_in = 0.0
    hp_prev_out = 0.0

    def filter_chunk(raw: list[int]) -> list[int]:
        nonlocal hp_prev_in, hp_prev_out
        filtered: list[int] = []
        # Stateful first-order DC blocker. State spans every emitted chunk,
        # preserving byte-identical output regardless of chunk size.
        for sample in raw:
            x = float(sample)
            y = x - hp_prev_in + 0.995 * hp_prev_out
            hp_prev_in = x
            hp_prev_out = y
            # Soft-knee limiter prevents DC-blocker transients from clipping
            # while preserving low-level articulation.
            knee = 0.80 * MAX_I16
            limit = 0.94 * MAX_I16
            ay = abs(y)
            if ay > knee:
                sign = -1.0 if y < 0.0 else 1.0
                y = sign * (knee + (limit - knee) * math.tanh((ay - knee) / (limit - knee)))
            value = int(round(y))
            filtered.append(max(MIN_I16, min(MAX_I16, value)))
        return filtered

    def drain(force: bool = False) -> Iterator[list[int]]:
        nonlocal ready
        while len(ready) >= chunk_frames:
            raw = ready[:chunk_frames]
            del ready[:chunk_frames]
            yield filter_chunk(raw)
        if force and ready:
            raw = list(ready)
            ready.clear()
            yield filter_chunk(raw)

    for idx, event in enumerate(events):
        seg = _segment(events, idx, profile)
        if not pending:
            pending = seg
            continue

        fade = min(_fade_frames(PHONES[event.symbol].kind), len(pending), len(seg))
        if fade:
            safe = pending[:-fade]
            blended = _crossfade_samples(pending, seg, fade)
            ready.extend(safe)
            pending = blended + seg[fade:]
        else:
            ready.extend(pending)
            pending = seg

        yield from drain(False)

    ready.extend(pending)
    yield from drain(True)

def synthesize(text: str, voice: str = "screen") -> list[int]:
    out: list[int] = []
    for chunk in synthesize_stream(text, voice):
        out.extend(chunk)
    return out

def pcm_s16le_mono(samples: Iterable[int]) -> bytes:
    return b"".join(struct.pack("<h", max(MIN_I16, min(MAX_I16, int(s)))) for s in samples)

def pcm_s16le_stereo(samples: Iterable[int]) -> bytes:
    out = bytearray()
    for sample in samples:
        s = max(MIN_I16, min(MAX_I16, int(sample)))
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
        return {
            "peak":0.0, "rms":0.0, "dc":0.0, "clip_ratio":0.0,
            "duration_s":0.0, "zcr":0.0,
        }
    peak = max(abs(s) for s in samples)
    rms = math.sqrt(sum(float(s) * s for s in samples) / len(samples))
    dc = abs(sum(samples) / len(samples))
    clips = sum(1 for s in samples if abs(s) >= int(MAX_I16 * 0.999))
    crossings = sum(1 for a,b in zip(samples, samples[1:]) if (a < 0 <= b) or (b < 0 <= a))
    return {
        "peak":peak / MAX_I16,
        "rms":rms / MAX_I16,
        "dc":dc / MAX_I16,
        "clip_ratio":clips / len(samples),
        "duration_s":len(samples) / SAMPLE_RATE,
        "zcr":crossings / max(1, len(samples) - 1),
    }

def engine_fingerprint() -> str:
    payload = (
        ENGINE_NAME + "|" + str(ENGINE_ABI) + "|" +
        "|".join(sorted(VOICES)) + "|" +
        "|".join(sorted(PHONES)) + "|" +
        "|".join(sorted(LEXICON))
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()

def render_reference_set(out_dir: str | Path) -> dict[str, str]:
    out_dir = Path(out_dir)
    phrase = (
        "Bonjour. UEFI, menu sécurité. Lecteur d'écran audio. "
        "Continuer, récupération, erreur, non. USB, PCI, HDA, TPM."
    )
    hashes: dict[str, str] = {}
    for voice in VOICES:
        samples = synthesize(phrase, voice)
        path = out_dir / f"voicecore-v4-{voice}.wav"
        write_wav(path, samples)
        hashes[voice] = hashlib.sha256(path.read_bytes()).hexdigest()
    return hashes

if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="VoiceCore v4 deterministic first-party French TTS")
    parser.add_argument("text", nargs="?", default="Bonjour. Synthèse vocale maison.")
    parser.add_argument("--voice", choices=sorted(VOICES), default="screen")
    parser.add_argument("--out", default="voicecore-v4.wav")
    args = parser.parse_args()
    samples = synthesize(args.text, args.voice)
    write_wav(args.out, samples)
    print(f"VOICECORE_V4=PASS voice={args.voice} samples={len(samples)} out={args.out}")
    print(f"fingerprint={engine_fingerprint()}")
    print(quality_metrics(samples))
