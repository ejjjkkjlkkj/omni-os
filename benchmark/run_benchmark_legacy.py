#!/usr/bin/env python3
"""
run_benchmark.py - Comparaison empirique ST v3 vs SAPI 5.

Etapes :
  1. Genere les WAV via st.exe (mesure wall-clock du temps de synthese)
  2. Genere les WAV via SAPI 5 (SpVoice + SpFileStream COM)
  3. Calcule des metriques objectives sur chaque WAV
  4. Genere un rapport HTML avec spectrogrammes SVG inline et tableau comparatif

Usage :
  python run_benchmark.py
"""
from __future__ import annotations

import ctypes
import json
import os
import shutil
import subprocess
import sys
import time
import wave
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import numpy as np
import scipy.signal as sps
import soundfile as sf

ROOT = Path(__file__).resolve().parent
ST_EXE = ROOT.parent / "target" / "release" / "st.exe"
WAV_DIR = ROOT / "wavs"
OUT_JSON = ROOT / "metrics.json"
OUT_HTML = ROOT / "rapport.html"

# ---------------------------------------------------------------------------
# Generation WAV
# ---------------------------------------------------------------------------

def gen_st_wav(text: str, lang: str, voice: str, out_path: Path) -> float:
    """Lance st.exe et renvoie le temps de synthese (ms)."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        str(ST_EXE),
        "--text", text,
        "--lang", lang,
        "--voice", voice,
        "--out", str(out_path),
    ]
    t0 = time.perf_counter()
    res = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    dt_ms = (time.perf_counter() - t0) * 1000.0
    if res.returncode != 0:
        raise RuntimeError(f"st.exe failed: {res.stderr.strip()}")
    if not out_path.exists():
        raise RuntimeError("st.exe n'a pas produit de WAV")
    return dt_ms


def gen_sapi_wav(text: str, voice_token: str, out_path: Path) -> None:
    """Genere un WAV via SAPI 5 via PowerShell COM (pas besoin de pywin32)."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    ps = ROOT / "sapi_gen.ps1"
    cmd = [
        "powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
        "-File", str(ps),
        "-VoiceName", voice_token,
        "-Text", text,
        "-OutFile", str(out_path),
    ]
    res = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    if res.returncode != 0:
        raise RuntimeError(f"sapi_gen.ps1 failed: {res.stderr.strip() or res.stdout.strip()}")


# ---------------------------------------------------------------------------
# Metriques audio
# ---------------------------------------------------------------------------

@dataclass
class AudioMetrics:
    path: str
    engine: str
    lang: str
    voice: str
    phrase_id: str
    text: str
    duration_s: float
    sample_rate: int
    channels: int
    samples: int
    peak_dbfs: float
    rms_dbfs: float
    crest_db: float
    dyn_range_db: float
    zcr_mean: float
    spec_centroid_hz: float
    spec_centroid_std: float
    spec_rolloff85_hz: float
    spec_bandwidth_hz: float
    f0_mean_hz: float
    f0_std_hz: float
    hnr_db: float
    voiced_ratio: float
    synth_ms: Optional[float] = None
    file_kb: int = 0
    error: Optional[str] = None


def _db(x: float, floor: float = 1e-12) -> float:
    return 20.0 * np.log10(max(x, floor))


def compute_metrics(path: Path, engine: str, lang: str, voice: str,
                    phrase_id: str, text: str,
                    synth_ms: Optional[float] = None) -> AudioMetrics:
    try:
        x, sr = sf.read(str(path))
        if x.ndim > 1:
            x = x.mean(axis=1)
        x = x.astype(np.float64)
        peak = float(np.max(np.abs(x)))
        rms = float(np.sqrt(np.mean(x ** 2)))
        peak_db = _db(peak)
        rms_db = _db(rms)
        crest = peak_db - rms_db
        # Plage dynamique approx (95e - 5e centile en dB sur fenetre 40ms)
        win = max(1, int(sr * 0.04))
        hop = max(1, win // 2)
        if len(x) > win:
            rms_frames = np.array([
                np.sqrt(np.mean(x[i:i + win] ** 2))
                for i in range(0, len(x) - win, hop)
            ])
            rms_db_frames = 20.0 * np.log10(np.maximum(rms_frames, 1e-7))
            dyn_range = float(np.percentile(rms_db_frames, 95) -
                              np.percentile(rms_db_frames, 5))
        else:
            dyn_range = 0.0

        # ZCR
        zcr = float(np.mean(np.abs(np.diff(np.sign(x))) > 0))

        # Spectre moyenne (FFT 2048)
        nperseg = 2048
        f, t, Sxx = sps.spectrogram(x, fs=sr, nperseg=nperseg,
                                    noverlap=nperseg // 2,
                                    window="hann")
        # eviter log(0)
        Sxx_n = Sxx / (np.max(Sxx) + 1e-12)
        col_sum = Sxx_n.sum(axis=0) + 1e-12
        centroid = (f[:, None] * Sxx_n).sum(axis=0) / col_sum
        spec_centroid_mean = float(np.mean(centroid))
        spec_centroid_std = float(np.std(centroid))
        # rolloff 85% (par frame, puis moyenne)
        cumsum = np.cumsum(Sxx_n, axis=0)
        idx = np.argmax(cumsum >= 0.85 * cumsum[-1:, :], axis=0)
        rolloff85 = float(np.mean(f[np.minimum(idx, len(f) - 1)]))
        # bandwidth : ecart-type autour du centroide par frame
        bw = float(np.mean(np.sqrt(((f[:, None] - centroid) ** 2) *
                                    Sxx_n).sum(axis=0) / col_sum))

        # F0 et HNR via autocorrel par frame voisee
        f0_vals, hnr_vals = [], []
        voiced_count = 0
        frame_len = int(sr * 0.04)
        frame_hop = int(sr * 0.01)
        lo, hi = 60, 400
        min_lag = int(sr / hi)
        max_lag = int(sr / lo)
        for i in range(0, len(x) - frame_len, frame_hop):
            frame = x[i:i + frame_len]
            fr = np.sqrt(np.mean(frame ** 2))
            if fr < 0.01 * (rms + 1e-9):
                continue
            voiced_count += 1
            xc = np.correlate(frame, frame, mode="full")
            xc = xc[len(frame) - 1:]
            if max_lag >= len(xc):
                continue
            seg = xc[min_lag:max_lag]
            if len(seg) == 0:
                continue
            # pic dans la zone F0
            k = int(np.argmax(seg))
            lag = min_lag + k
            f0 = sr / lag if lag > 0 else 0.0
            f0_vals.append(f0)
            r_max = seg[k] / (xc[0] + 1e-12)
            hnr_vals.append(_db(r_max / (1.0 - r_max + 1e-12)))

        if f0_vals:
            f0_mean = float(np.mean(f0_vals))
            f0_std = float(np.std(f0_vals))
        else:
            f0_mean, f0_std = 0.0, 0.0
        hnr_db = float(np.mean(hnr_vals)) if hnr_vals else 0.0
        total_frames = max(1, (len(x) - frame_len) // frame_hop)
        voiced_ratio = voiced_count / total_frames

        size_kb = path.stat().st_size // 1024
        return AudioMetrics(
            path=str(path),
            engine=engine, lang=lang, voice=voice,
            phrase_id=phrase_id, text=text,
            duration_s=len(x) / sr,
            sample_rate=sr,
            channels=1,
            samples=len(x),
            peak_dbfs=peak_db,
            rms_dbfs=rms_db,
            crest_db=crest,
            dyn_range_db=dyn_range,
            zcr_mean=zcr,
            spec_centroid_hz=spec_centroid_mean,
            spec_centroid_std=spec_centroid_std,
            spec_rolloff85_hz=rolloff85,
            spec_bandwidth_hz=bw,
            f0_mean_hz=f0_mean,
            f0_std_hz=f0_std,
            hnr_db=hnr_db,
            voiced_ratio=voiced_ratio,
            synth_ms=synth_ms,
            file_kb=size_kb,
        )
    except Exception as e:
        return AudioMetrics(
            path=str(path), engine=engine, lang=lang, voice=voice,
            phrase_id=phrase_id, text=text,
            duration_s=0, sample_rate=0, channels=0, samples=0,
            peak_dbfs=0, rms_dbfs=0, crest_db=0, dyn_range_db=0,
            zcr_mean=0, spec_centroid_hz=0, spec_centroid_std=0,
            spec_rolloff85_hz=0, spec_bandwidth_hz=0,
            f0_mean_hz=0, f0_std_hz=0, hnr_db=0, voiced_ratio=0,
            synth_ms=synth_ms, file_kb=0, error=str(e),
        )


# ---------------------------------------------------------------------------
# Spectrogramme SVG
# ---------------------------------------------------------------------------

def render_spectrogram_svg(path: Path, width: int = 480, height: int = 120,
                           max_freq: int = 8000) -> str:
    """Spectrogramme SVG en niveaux de gris (pas de matplotlib requis)."""
    try:
        x, sr = sf.read(str(path))
        if x.ndim > 1:
            x = x.mean(axis=1)
        x = x.astype(np.float64)
        if len(x) < 1024:
            return f'<svg width="{width}" height="{height}"></svg>'
        nperseg = 1024
        noverlap = nperseg // 2
        f, t, Sxx = sps.spectrogram(x, fs=sr, nperseg=nperseg,
                                    noverlap=noverlap, window="hann")
        f_mask = f <= max_freq
        f = f[f_mask]
        Sxx = Sxx[f_mask]
        # log + normalisation
        S = 20.0 * np.log10(Sxx + 1e-9)
        S = np.clip((S - S.max() + 60) / 60.0, 0, 1)  # 0=noir,1=blanc
        n_freq, n_time = S.shape
        # SVG : chaque pixel = un <rect>
        rect_w = width / n_time
        rect_h = height / n_freq
        rects = []
        for j in range(n_freq):
            # inverser pour que les basses soient en bas
            row = S[n_freq - 1 - j]
            for i in range(n_time):
                v = row[i]
                gray = int(255 * v)
                rects.append(
                    f'<rect x="{i * rect_w:.2f}" y="{j * rect_h:.2f}" '
                    f'width="{rect_w + 0.5:.2f}" height="{rect_h + 0.5:.2f}" '
                    f'fill="rgb({gray},{gray},{gray})"/>'
                )
        svg = (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" '
            f'height="{height}" viewBox="0 0 {width} {height}" '
            f'shape-rendering="crispEdges">'
            f'<rect width="{width}" height="{height}" fill="black"/>'
            + "".join(rects)
            + "</svg>"
        )
        return svg
    except Exception as e:
        return f'<svg width="{width}" height="{height}"><text x="4" y="20" fill="red">err: {e}</text></svg>'


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------

def main() -> int:
    if not ST_EXE.exists():
        print(f"ERREUR : {ST_EXE} introuvable. Lance d'abord cargo build --release.",
              file=sys.stderr)
        return 1
    if WAV_DIR.exists():
        shutil.rmtree(WAV_DIR)
    WAV_DIR.mkdir(parents=True)

    corpus = json.loads((ROOT / "corpus.json").read_text(encoding="utf-8"))

    voices_st = {"fr": "female", "en": "female"}  # default voice ST
    voices_sapi = {"fr": "Hortense", "en": "Zira"}

    results: List[AudioMetrics] = []
    total = sum(len(v) for v in corpus["languages"].values())
    done = 0

    for lang, phrases in corpus["languages"].items():
        for ph in phrases:
            done += 1
            pid = ph["id"]
            txt = ph["text"]
            print(f"[{done:2d}/{total}] {pid} ... ", end="", flush=True)

            # ---- ST ----
            st_wav = WAV_DIR / "st" / lang / f"{pid}.wav"
            try:
                t_ms = gen_st_wav(txt, lang, voices_st[lang], st_wav)
                m_st = compute_metrics(st_wav, "ST v3", lang, voices_st[lang],
                                       pid, txt, synth_ms=t_ms)
                results.append(m_st)
                print(f"ST {t_ms:6.1f}ms  | ", end="")
            except Exception as e:
                print(f"ST ERR: {e}  | ", end="")

            # ---- SAPI 5 ----
            sapi_wav = WAV_DIR / "sapi5" / lang / f"{pid}.wav"
            try:
                gen_sapi_wav(txt, voices_sapi[lang], sapi_wav)
                m_sapi = compute_metrics(sapi_wav, "SAPI 5", lang, voices_sapi[lang],
                                         pid, txt)
                results.append(m_sapi)
                print(f"SAPI OK ({m_sapi.duration_s:.2f}s)")
            except Exception as e:
                print(f"SAPI ERR: {e}")

    OUT_JSON.write_text(
        json.dumps([asdict(r) for r in results], indent=2, ensure_ascii=False),
        encoding="utf-8")
    print(f"\nMetriques sauvegardees -> {OUT_JSON}")
    return 0


if __name__ == "__main__":
    sys.exit(main())