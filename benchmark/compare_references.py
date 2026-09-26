"""Signal-level comparison of a benchmark run against the local JAWS/Vocalizer bank.

The reference captures have unknown text, so only text-independent measures are
compared (HNR, F0 behaviour, spectral centroid, bandwidth, noise floor, dynamics).
Reference WAVs are read in place and never copied or redistributed.

Usage: python compare_references.py --run nextgen [--refs DIR]
"""
from pathlib import Path
import argparse, json
import numpy as np
import soundfile as sf
import scipy.signal as signal
from run_benchmark import analyze

ROOT = Path(__file__).resolve().parent
KEYS = ['hnr_db', 'jitter_local_pct', 'f0_std_hz', 'f0_step_p95_st', 'spec_centroid_hz',
        'bandwidth_hz', 'noise_floor_db', 'active_dynamic_db', 'voiced_ratio', 'crest_db']

def extra(path):
    """-40 dB spectral bandwidth and noise floor, measured on the file's native rate."""
    x, sr = sf.read(path, always_2d=True)
    x = x.mean(axis=1)
    f, p = signal.welch(x, fs=sr, nperseg=4096)
    pdb = 10*np.log10(np.maximum(p, 1e-30))
    top = pdb.max()
    bandwidth = float(f[np.nonzero(pdb > top-40)[0].max()])
    frames = np.lib.stride_tricks.sliding_window_view(x, sr//50)[::sr//100]
    lv = 20*np.log10(np.maximum(np.sqrt(np.mean(frames**2, axis=1)), 1e-9))
    return dict(bandwidth_hz=bandwidth, noise_floor_db=float(np.percentile(lv, 5) - lv.max()))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--run', required=True)
    ap.add_argument('--refs', type=Path, default=ROOT.parent/'work/jaws-direct-profile-bank/20260926-215235')
    a = ap.parse_args()
    run = ROOT/'runs'/a.run
    records = [r for r in json.loads((run/'metrics.json').read_text(encoding='utf-8')) if r['status'] == 'PASS']
    for r in records:
        r.update(extra(ROOT/r['path']))
    refs = []
    for wav in sorted(a.refs.glob('*.wav')):
        r = analyze(wav)
        r.update(extra(wav), engine='jaws-vocalizer', path=str(wav), sha256=r['sha256'])
        refs.append(r)
    groups = {}
    for r in records + refs:
        groups.setdefault(r['engine'], []).append(r)
    summary = {}
    for engine, rows in groups.items():
        s = {'n': len(rows)}
        for k in KEYS + ['engine_first_audio_ms', 'engine_rtf', 'engine_load_ms']:
            v = [r[k] for r in rows if r.get(k) is not None]
            if v:
                s[k] = float(np.median(v))
        summary[engine] = s
    ref = summary.get('jaws-vocalizer', {})
    # Distance to reference: mean |z| over text-independent keys, scaled by reference spread.
    spread = {k: float(np.std([r[k] for r in refs if r.get(k) is not None]) or 1.0) for k in KEYS}
    for engine, s in summary.items():
        d = [abs(s[k]-ref[k])/max(spread[k], abs(ref[k])*0.05, 1e-6) for k in KEYS if k in s and k in ref]
        s['distance_to_reference'] = float(np.mean(d)) if d else None
    (run/'reference-comparison.json').write_text(json.dumps(dict(summary=summary, references=refs), indent=2), encoding='utf-8')
    cols = ['n'] + KEYS + ['engine_first_audio_ms', 'engine_rtf', 'distance_to_reference']
    lines = ['| engine | ' + ' | '.join(cols) + ' |', '|' + '---|'*(len(cols)+1)]
    for engine, s in summary.items():
        lines.append(f'| {engine} | ' + ' | '.join(f'{s[c]:.3g}' if isinstance(s.get(c), float) else str(s.get(c, '')) for c in cols) + ' |')
    table = '\n'.join(lines)
    (run/'reference-comparison.md').write_text(table + '\n', encoding='utf-8')
    print(table)

if __name__ == '__main__':
    main()
