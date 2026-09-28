#!/usr/bin/env python3
"""
build_report.py - Construit le rapport HTML de comparaison ST v3 vs SAPI 5.

Lit metrics.json + wavs/, produit :
  - rapport.html : tableau comparatif + spectrogrammes SVG inline
  - abx.html     : page de test ABX (evaluation subjective en aveugle)

Chaque ligne du tableau :
  phrase | texte | ST (duree, RMS, F0, HNR, taille, synth_ms, spectrogramme)
                    | SAPI 5 (memes metriques)
"""
from __future__ import annotations

import html
import json
import math
import struct
import urllib.parse
import zlib
from pathlib import Path
from typing import Dict, List, Tuple

import numpy as np
import scipy.signal as sps
import soundfile as sf


# ---------------------------------------------------------------------------
# Encodeur PNG minimaliste en pur Python (zlib + struct)
# ---------------------------------------------------------------------------

def _png_chunk(tag: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)


def array_to_png_data_uri(gray: np.ndarray) -> str:
    """Encode une matrice 2D float [0..1] en PNG 8-bit grayscale data URI."""
    h, w = gray.shape
    g8 = np.clip(gray * 255.0 + 0.5, 0, 255).astype(np.uint8)
    raw = b"".join(b"\x00" + g8[i].tobytes() for i in range(h))  # filtre "None"
    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)  # 8-bit, grayscale
    idat = zlib.compress(raw, 9)
    png = sig + _png_chunk(b"IHDR", ihdr) + _png_chunk(b"IDAT", idat) + _png_chunk(b"IEND", b"")
    b64 = __import__("base64").b64encode(png).decode("ascii")
    return "data:image/png;base64," + b64


def compute_spectrogram_png(path: Path, width: int = 320, height: int = 80,
                            max_freq: int = 8000) -> str:
    """Spectrogramme PNG (downsampled) -> data URI."""
    try:
        x, sr = sf.read(str(path))
        if x.ndim > 1:
            x = x.mean(axis=1)
        x = x.astype(np.float64)
        if len(x) < 1024:
            return ""
        nperseg = 2048
        noverlap = nperseg // 2
        f, t, Sxx = sps.spectrogram(x, fs=sr, nperseg=nperseg,
                                    noverlap=noverlap, window="hann")
        f_mask = f <= max_freq
        f = f[f_mask]; Sxx = Sxx[f_mask]
        S = 20.0 * np.log10(Sxx + 1e-9)
        S = np.clip((S - S.max() + 60) / 60.0, 0, 1)
        # Downsample a (height, width) par moyenne par blocs
        n_freq, n_time = S.shape
        out = np.zeros((height, width), dtype=np.float32)
        for i in range(height):
            i0 = (i * n_freq) // height
            i1 = max(i0 + 1, ((i + 1) * n_freq) // height)
            for j in range(width):
                j0 = (j * n_time) // width
                j1 = max(j0 + 1, ((j + 1) * n_time) // width)
                out[i, j] = S[i0:i1, j0:j1].mean()
        # Inverser pour basses en bas
        out = out[::-1]
        return array_to_png_data_uri(out)
    except Exception as e:
        # PNG 1x1 noir en cas d'erreur
        black = np.zeros((1, 1), dtype=np.float32)
        return array_to_png_data_uri(black)

ROOT = Path(__file__).resolve().parent
METRICS = ROOT / "metrics.json"
WAV_DIR = ROOT / "wavs"
OUT_HTML = ROOT / "rapport.html"
OUT_ABX = ROOT / "abx.html"
CORPUS = json.loads((ROOT / "corpus.json").read_text(encoding="utf-8"))


def render_spectrogram_html(path: Path, width: int = 320, height: int = 80) -> str:
    """Spectrogramme rendu en PNG inline (data URI)."""
    uri = compute_spectrogram_png(path, width, height)
    return (f'<img class="spectro" src="{uri}" width="{width}" height="{height}" '
            f'alt="spectrogramme {path.stem}"/>')


def fmt(v, fmt_spec: str, fallback: str = "—") -> str:
    if v is None or v == 0 or (isinstance(v, float) and (math.isnan(v) or math.isinf(v))):
        return fallback
    return format(v, fmt_spec)


def build_html(metrics: List[dict]) -> str:
    # Grouper par phrase_id
    by_phrase: Dict[str, Dict[str, dict]] = {}
    for m in metrics:
        by_phrase.setdefault(m["phrase_id"], {})[m["engine"]] = m

    # Stats agregees par moteur
    agg: Dict[str, Dict[str, list]] = {}
    for m in metrics:
        if m.get("error"):
            continue
        agg.setdefault(m["engine"], []).append(m)

    # Calcul du "vainqueur" par phrase sur quelques metriques cles
    rows = []
    for pid, engines in by_phrase.items():
        st = engines.get("ST v3")
        sapi = engines.get("SAPI 5")
        if not st or not sapi:
            continue
        rows.append((pid, st, sapi))

    # Resume agrege
    summary_rows = ""
    for engine, items in agg.items():
        n = len(items)
        avg_dur = sum(i["duration_s"] for i in items) / n
        avg_rms = sum(i["rms_dbfs"] for i in items) / n
        avg_peak = sum(i["peak_dbfs"] for i in items) / n
        avg_centroid = sum(i["spec_centroid_hz"] for i in items) / n
        avg_f0 = sum(i["f0_mean_hz"] for i in items) / n
        avg_f0_std = sum(i["f0_std_hz"] for i in items) / n
        avg_hnr = sum(i["hnr_db"] for i in items) / n
        avg_voiced = sum(i["voiced_ratio"] for i in items) / n
        avg_size = sum(i["file_kb"] for i in items) / n
        synth = [i["synth_ms"] for i in items if i.get("synth_ms") is not None]
        avg_synth = sum(synth) / len(synth) if synth else None
        summary_rows += f"""
        <tr>
          <td><b>{html.escape(engine)}</b></td>
          <td>{n}</td>
          <td>{fmt(avg_dur, '.2f')} s</td>
          <td>{fmt(avg_rms, '.2f')} dBFS</td>
          <td>{fmt(avg_peak, '.2f')} dBFS</td>
          <td>{fmt(avg_centroid, '.0f')} Hz</td>
          <td>{fmt(avg_f0, '.1f')} Hz</td>
          <td>{fmt(avg_f0_std, '.1f')} Hz</td>
          <td>{fmt(avg_hnr, '.2f')} dB</td>
          <td>{fmt(avg_voiced * 100, '.1f')} %</td>
          <td>{fmt(avg_size, '.1f')} ko</td>
          <td>{fmt(avg_synth, '.1f')} ms</td>
        </tr>"""

    # Comparaison par phrase
    detail_rows = ""
    for pid, st, sapi in rows:
        # chemin relatif pour servir les WAV depuis le rapport
        st_wav_rel = Path(st["path"]).relative_to(ROOT).as_posix()
        sapi_wav_rel = Path(sapi["path"]).relative_to(ROOT).as_posix()
        st_svg = render_spectrogram_html(Path(st["path"]))
        sapi_svg = render_spectrogram_html(Path(sapi["path"]))
        # ratio vitesse synthese/audio
        st_speed = (st["synth_ms"] / 1000.0) / st["duration_s"] if st.get("synth_ms") else None
        speed_str = f"{st_speed:.3f}x" if st_speed is not None else "—"
        # ratio HNR (indicateur qualite)
        hnr_diff = st["hnr_db"] - sapi["hnr_db"]
        hnr_class = "st" if hnr_diff > 0 else "sapi"
        # ratio taille memoire
        size_ratio = st["file_kb"] / sapi["file_kb"] if sapi["file_kb"] else 0
        detail_rows += f"""
        <tr>
          <td class="pid">{html.escape(pid)}<br><span class="cat">{html.escape(st.get('category') or st.get('engine','?') or '')}</span></td>
          <td class="text">« {html.escape(st["text"])} »</td>
          <td>
            <div class="spectro">{st_svg}</div>
            <audio controls preload="none" src="{html.escape(st_wav_rel)}"></audio>
          </td>
          <td>
            <div class="spectro">{sapi_svg}</div>
            <audio controls preload="none" src="{html.escape(sapi_wav_rel)}"></audio>
          </td>
          <td class="metric">
            <div><b>ST</b> : {fmt(st["duration_s"], '.2f')}s · {fmt(st["rms_dbfs"], '.1f')} dBFS</div>
            <div><b>SAPI</b> : {fmt(sapi["duration_s"], '.2f')}s · {fmt(sapi["rms_dbfs"], '.1f')} dBFS</div>
          </td>
          <td class="metric">
            <div><b>ST</b> : {fmt(st["f0_mean_hz"], '.1f')} Hz (σ={fmt(st["f0_std_hz"], '.1f')})</div>
            <div><b>SAPI</b> : {fmt(sapi["f0_mean_hz"], '.1f')} Hz (σ={fmt(sapi["f0_std_hz"], '.1f')})</div>
          </td>
          <td class="metric {hnr_class}">
            <div><b>ST</b> : {fmt(st["hnr_db"], '.2f')} dB</div>
            <div><b>SAPI</b> : {fmt(sapi["hnr_db"], '.2f')} dB</div>
            <div class="diff">Δ = {hnr_diff:+.2f} dB</div>
          </td>
          <td class="metric">
            <div><b>ST</b> : {fmt(st["spec_centroid_hz"], '.0f')} Hz</div>
            <div><b>SAPI</b> : {fmt(sapi["spec_centroid_hz"], '.0f')} Hz</div>
          </td>
          <td class="metric">
            <div><b>ST</b> : {fmt(st["file_kb"], '.1f')} ko</div>
            <div><b>SAPI</b> : {fmt(sapi["file_kb"], '.1f')} ko</div>
            <div class="diff">ratio = {size_ratio:.2f}x</div>
          </td>
          <td class="metric speed">
            <div><b>ST</b> : {fmt(st["synth_ms"], '.1f')} ms ({speed_str})</div>
            <div><b>SAPI</b> : non exposé (boîte noire)</div>
          </td>
        </tr>"""

    # Indisponibilite documentee
    unavailable = r"""
    <details class="unavailable">
      <summary>Moteurs listes mais non utilisables en CLI sur cette machine</summary>
      <table>
        <tr><th>Moteur</th><th>Statut</th><th>Détail</th></tr>
        <tr>
          <td>eSpeak NG</td><td>indisponible</td>
          <td>Aucun <code>espeak-ng.exe</code> autonome installé. Présent
          uniquement comme bibliothèque embarquée dans NVDA
          (<code>C:\Program Files\NVDA\synthDrivers\espeak.dll</code>),
          non exposée en CLI.</td>
        </tr>
        <tr>
          <td>Microsoft OneCore voices</td><td>indisponible</td>
          <td>L'énumération <code>[Windows.Media.SpeechSynthesis]</code> retourne 0 voix
          : le pack optionnel « Voix » n'est pas installé sur ce poste.
          Même via <code>System.Speech</code> les voix OneCore ne sont pas listées.</td>
        </tr>
        <tr>
          <td>Ivona (Amazon Polly)</td><td>indisponible</td>
          <td>Produit commercial AWS. Aucune trace d'installation locale
          (registry, dossier Program Files). Accès uniquement via API cloud.</td>
        </tr>
        <tr>
          <td>Vocalizer Expressive 2.2 (Nuance)</td><td>indisponible</td>
          <td>Synthèse Nuance, livrée en bundle avec JAWS. Aucune trace
          d'installation locale.</td>
        </tr>
      </table>
      <p><b>Conclusion :</b> la comparaison empirique n'a pu être menée qu'entre
      <b>ST v3</b> et <b>SAPI 5 (Microsoft Hortense / Zira)</b>, les seuls moteurs
      réellement accessibles et pilotables en CLI sur cette machine.</p>
    </details>"""

    css = """
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
           margin: 0; padding: 20px; background: #0d1117; color: #e6edf3; }
    h1, h2, h3 { color: #58a6ff; }
    h1 { border-bottom: 2px solid #58a6ff; padding-bottom: 8px; }
    table { border-collapse: collapse; width: 100%; margin: 12px 0; background: #161b22; }
    th, td { border: 1px solid #30363d; padding: 8px; vertical-align: top; }
    th { background: #21262d; color: #79c0ff; text-align: left; font-weight: 600; }
    tr:hover td { background: #1c2128; }
    td.pid { font-family: monospace; color: #d2a8ff; white-space: nowrap; }
    td.text { font-style: italic; color: #f0f6fc; max-width: 220px; }
    td.metric { font-family: monospace; font-size: 12px; line-height: 1.45; min-width: 130px; }
    td.metric.st { background: rgba(46,160,67,0.10); }
    td.metric.sapi { background: rgba(248,81,73,0.10); }
    td.metric.speed { color: #79c0ff; }
    .cat { font-size: 10px; color: #8b949e; }
    .diff { color: #f0883e; font-size: 11px; }
    .spectro { display: block; width: 320px; max-width: 100%; height: 80px;
               background: #000; border: 1px solid #30363d; border-radius: 4px; }
    audio { width: 320px; max-width: 100%; margin-top: 4px; height: 28px; }
    .unavailable { background: #161b22; border-left: 4px solid #f0883e; padding: 12px 16px; margin: 16px 0; }
    .legend { display: flex; gap: 16px; font-size: 13px; margin: 12px 0; }
    .legend .sw { display: inline-block; width: 14px; height: 14px;
                  vertical-align: middle; margin-right: 4px; border-radius: 3px; }
    .sw.st { background: rgba(46,160,67,0.25); border: 1px solid #2ea043; }
    .sw.sapi { background: rgba(248,81,73,0.25); border: 1px solid #f85149; }
    code { background: #0d1117; color: #79c0ff; padding: 1px 6px; border-radius: 3px; }
    .stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
                 gap: 12px; margin: 16px 0; }
    .stat-grid .card { background: #161b22; border: 1px solid #30363d; border-radius: 6px;
                       padding: 12px 16px; }
    .stat-grid .card .v { font-size: 26px; font-weight: 700; color: #58a6ff; }
    .stat-grid .card .l { font-size: 12px; color: #8b949e; text-transform: uppercase; letter-spacing: 0.5px; }
    .stat-grid .card.win { border-color: #2ea043; }
    .stat-grid .card.lose { border-color: #f85149; }
    """

    head = f"""<!DOCTYPE html>
<html lang="fr"><head><meta charset="utf-8">
<title>ST v3 vs SAPI 5 — Preuve empirique</title>
<style>{css}</style></head><body>
<h1>🎙️ ST v3 (formants Klatt, Rust) vs SAPI 5 — Rapport de comparaison</h1>
<p>Généré le {html.escape(__import__('datetime').datetime.now().isoformat(timespec='seconds'))} ·
   corpus : {sum(len(v) for v in CORPUS['languages'].values())} phrases ·
   2 moteurs pilotés en CLI · {len(metrics)} mesures.</p>

{unavailable}

<h2>Résumé agrégé (moyennes sur tout le corpus)</h2>
<div class="legend">
  <span><span class="sw st"></span> Surbrillance verte : ST gagne sur la métrique</span>
  <span><span class="sw sapi"></span> Surbrillance rouge : SAPI gagne</span>
</div>
<table>
  <tr>
    <th>Moteur</th><th>N</th><th>Durée</th><th>RMS</th><th>Pic</th>
    <th>Centroïde spectral</th><th>F0 moyen</th><th>σ F0</th><th>HNR</th>
    <th>% voisé</th><th>Taille WAV</th><th>Temps synthèse</th>
  </tr>
  {summary_rows}
</table>
"""

    body = f"""
<h2>Comparaison phrase par phrase</h2>
<table>
  <tr>
    <th>ID</th><th>Texte</th><th>Spectrogrammes (ST · SAPI)</th>
    <th>Durée · RMS</th><th>F0 · jitter</th>
    <th>HNR (harmonicité)</th><th>Centroïde spectral</th>
    <th>Taille WAV</th><th>Temps de synthèse</th>
  </tr>
  {detail_rows}
</table>

<h2>Métriques utilisées</h2>
<ul>
  <li><b>RMS (dBFS)</b> : énergie moyenne sur signal complet. Plus haut = voix plus puissante.</li>
  <li><b>F0 moyen / σ F0</b> : pitch fondamental et variabilité (Hz). Une voix plus
      stable (σ faible) paraît plus naturelle ; σ trop faible = robotique monotone.</li>
  <li><b>HNR (Harmonic-to-Noise Ratio, dB)</b> : rapport harmoniques/bruit. Indicateur
      classique de qualité vocale. Plus haut = voix plus claire, moins bruitée.</li>
  <li><b>Centroïde spectral (Hz)</b> : centre de gravité du spectre. Plus haut = voix plus brillante.</li>
  <li><b>Taille WAV (ko)</b> : empreinte mémoire du PCM.</li>
  <li><b>Temps de synthèse (ms)</b> : uniquement pour ST (boîte noire pour SAPI).
      Mesuré via <code>time.perf_counter</code> autour de l'appel <code>st.exe</code>.</li>
</ul>

<p><i>Méthodologie complète et code source dans
<code>C:\st\benchmark\</code> — <code>run_benchmark.py</code>, <code>build_report.py</code>,
<code>sapi_gen.ps1</code>, <code>corpus.json</code>, <code>metrics.json</code>.</i></p>
</body></html>"""

    return head + body


def build_abx(metrics: List[dict]) -> str:
    """Page de test ABX : l'auditeur devine quel échantillon (A ou B) est ST."""
    by_phrase = {}
    for m in metrics:
        by_phrase.setdefault(m["phrase_id"], {})[m["engine"]] = m

    items = []
    import random
    rng = random.Random(42)
    for pid, engines in by_phrase.items():
        st = engines.get("ST v3")
        sapi = engines.get("SAPI 5")
        if not st or not sapi:
            continue
        # X est toujours ST, A et B sont randomisés
        st_rel = Path(st["path"]).relative_to(ROOT).as_posix()
        sapi_rel = Path(sapi["path"]).relative_to(ROOT).as_posix()
        if rng.random() < 0.5:
            a_rel, b_rel, label = st_rel, sapi_rel, "A"
        else:
            a_rel, b_rel, label = sapi_rel, st_rel, "B"
        items.append({
            "pid": pid,
            "text": st["text"],
            "category": st.get("category", ""),
            "a": a_rel,
            "b": b_rel,
            "x_is": label,  # X = A => A est ST ; X = B => B est ST
        })

    cards = ""
    for i, it in enumerate(items, 1):
        cards += f"""
        <div class="card" data-pid="{html.escape(it['pid'])}" data-x="{it['x_is']}">
          <h3>#{i} — {html.escape(it['category'])}</h3>
          <p class="text">« {html.escape(it['text'])} »</p>
          <div class="btns">
            <button class="play" data-src="{html.escape(it['a'])}" data-which="A">▶ A</button>
            <button class="play" data-src="{html.escape(it['b'])}" data-which="B">▶ B</button>
            <button class="play x" data-src="{html.escape(it['a'] if it['x_is']=='A' else it['b'])}" data-which="X">▶ X</button>
          </div>
          <div class="vote">
            X ressemble à : <button class="v" data-v="A">A</button>
            <button class="v" data-v="B">B</button>
            <span class="result"></span>
          </div>
        </div>"""

    js = """
    let cur = null;
    document.body.addEventListener('click', e => {
        if (e.target.classList.contains('play')) {
            const src = e.target.dataset.src;
            if (cur) { cur.pause(); cur.currentTime = 0; }
            cur = new Audio(src);
            cur.play();
        }
        if (e.target.classList.contains('v')) {
            const card = e.target.closest('.card');
            const guess = e.target.dataset.v;
            const truth = card.dataset.x;
            const ok = guess === truth;
            card.querySelector('.result').innerHTML =
                ok ? '<b style="color:#2ea043">✓ Correct (X=ST)</b>'
                   : '<b style="color:#f85149">✗ Faux (X=' + truth + ')</b>';
        }
      });
    """
    return f"""<!DOCTYPE html>
<html lang="fr"><head><meta charset="utf-8">
<title>Test ABX — ST v3 vs SAPI 5</title>
<style>
  body {{ font-family: -apple-system, sans-serif; background: #0d1117; color: #e6edf3;
          margin: 0; padding: 20px; }}
  h1 {{ color: #58a6ff; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
           gap: 16px; margin-top: 16px; }}
  .card {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px;
           padding: 14px 16px; }}
  .card h3 {{ margin: 0 0 4px; color: #79c0ff; font-size: 14px; }}
  .card .text {{ font-style: italic; color: #f0f6fc; margin: 4px 0 12px; }}
  .btns button {{ background: #21262d; color: #e6edf3; border: 1px solid #30363d;
                  padding: 6px 12px; border-radius: 4px; cursor: pointer; margin-right: 6px; }}
  .btns button:hover {{ background: #30363d; }}
  .btns button.x {{ border-color: #58a6ff; }}
  .vote {{ margin-top: 12px; font-size: 13px; color: #8b949e; }}
  .vote .v {{ background: #21262d; color: #e6edf3; border: 1px solid #30363d;
              padding: 4px 10px; border-radius: 4px; cursor: pointer; margin: 0 4px; }}
  .vote .v:hover {{ background: #30363d; }}
  .vote .result {{ margin-left: 10px; }}
  .instructions {{ background: #161b22; border-left: 4px solid #58a6ff; padding: 12px 16px; }}
</style></head><body>
<h1>🧪 Test ABX : ST v3 vs SAPI 5</h1>
<div class="instructions">
  <p>Écoutez A, puis B, puis X. X est identique à l'un des deux. Lequel ?</p>
  <p><b>But :</b> évaluer si la voix de ST v3 est identifiable en aveugle par un humain.</p>
  <p><i>Si l'auditeur ne peut pas dépasser 50 % de réussite, ST est indistinguishable
     de SAPI sur le critère testé (ici : timbre perçu).</i></p>
</div>
<div class="grid">{cards}</div>
<script>{js}</script>
</body></html>"""


def main():
    metrics = json.loads(METRICS.read_text(encoding="utf-8"))
    OUT_HTML.write_text(build_html(metrics), encoding="utf-8")
    OUT_ABX.write_text(build_abx(metrics), encoding="utf-8")
    print(f"Rapport : {OUT_HTML}")
    print(f"ABX     : {OUT_ABX}")


if __name__ == "__main__":
    main()