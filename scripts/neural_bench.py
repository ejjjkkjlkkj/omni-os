"""Stage-by-stage neural latency benchmark for screen-reader use.

One configuration per process (so imports/session creation are measured cold):
    python scripts/neural_bench.py --model int8 --provider cpu --threads 4 --out result.json

Stages: python import, ONNX session, voices, warm-up, then per utterance:
normalize (ST frontend is in Rust, so this is the worker's chunk planning),
phonemize (eSpeak), inference, resample -> first playable chunk (TTFA),
total, RTF. Reports P50/P90/P95/P99 and peak working set.
"""
import argparse, ctypes, json, os, statistics, sys, time
from pathlib import Path

T0 = time.perf_counter()
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'neural'))

CORPUS = {
    'fr-fr': ['Bouton', 'Menu', 'OK', 'Annuler', 'Case cochée', 'Menu Fichier', 'Explorateur de fichiers',
              'Paramètres', 'Zone de texte', 'Enregistrer, bouton.', 'Lien, Accueil.',
              'C deux points, Utilisateurs, adm, Documents.', 'Trois cent quarante-deux éléments.',
              'Voulez-vous enregistrer les modifications ?',
              "L'intelligence artificielle transforme profondément notre manière de travailler."],
    'en-us': ['Button', 'Menu', 'OK', 'Cancel', 'Checked', 'File menu', 'File Explorer', 'Settings',
              'Edit box', 'Save, button.', 'Link, Home.', 'Three hundred forty-two items.',
              'Do you want to save your changes?',
              'Artificial intelligence is fundamentally changing how we work.'],
}
VOICE = {'fr-fr': 'ff_siwis', 'en-us': 'af_heart'}


def peak_mb():
    class PMC(ctypes.Structure):
        _fields_ = [('cb', ctypes.c_ulong), ('PageFaultCount', ctypes.c_ulong)] + \
                   [(n, ctypes.c_size_t) for n in ('PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                    'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage', 'QuotaNonPagedPoolUsage',
                    'PagefileUsage', 'PeakPagefileUsage')]
    c = PMC(); c.cb = ctypes.sizeof(c)
    ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.windll.kernel32.GetCurrentProcess(), ctypes.byref(c), c.cb)
    return c.PeakWorkingSetSize / 2**20


def pct(values, p):
    v = sorted(values)
    return v[min(len(v) - 1, int(round(p / 100 * (len(v) - 1))))]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--model', choices=('fp32', 'int8'), default='fp32')
    ap.add_argument('--provider', choices=('cpu', 'dml'), default='cpu')
    ap.add_argument('--threads', type=int, default=4)
    ap.add_argument('--repeat', type=int, default=3)
    ap.add_argument('--out', type=Path)
    a = ap.parse_args()
    import numpy as np
    import onnxruntime as rt
    from kokoro_onnx import Kokoro
    from worker import plan, upsample2
    t_import = time.perf_counter() - T0

    models = ROOT / 'neural' / 'models'
    path = models / ('kokoro-v1.0.onnx' if a.model == 'fp32' else 'kokoro-v1.0.int8.onnx')
    o = rt.SessionOptions()
    o.intra_op_num_threads = a.threads
    o.inter_op_num_threads = 1
    o.graph_optimization_level = rt.GraphOptimizationLevel.ORT_ENABLE_ALL
    providers = ['CPUExecutionProvider'] if a.provider == 'cpu' else ['DmlExecutionProvider', 'CPUExecutionProvider']
    if a.provider == 'dml':
        o.enable_mem_pattern = False  # required by DirectML
        o.execution_mode = rt.ExecutionMode.ORT_SEQUENTIAL
    t = time.perf_counter()
    session = rt.InferenceSession(str(path), sess_options=o, providers=providers)
    t_session = time.perf_counter() - t
    t = time.perf_counter()
    k = Kokoro.from_session(session, str(models / 'voices-v1.0.bin'))
    t_voices = time.perf_counter() - t
    t = time.perf_counter()
    k.create('Bonjour.', voice='ff_siwis', lang='fr-fr')
    k.create('Hello.', voice='af_heart', lang='en-us')
    t_warm = time.perf_counter() - t

    rows = []
    for rep in range(a.repeat):
        for lang, texts in CORPUS.items():
            for text in texts:
                t = time.perf_counter()
                first = plan(text)[0][0]
                t_plan = time.perf_counter() - t
                t = time.perf_counter()
                ph = k.tokenizer.phonemize(first, lang)
                t_ph = time.perf_counter() - t
                t = time.perf_counter()
                samples, sr = k.create(ph, voice=VOICE[lang], lang=lang, is_phonemes=True)
                t_inf = time.perf_counter() - t
                t = time.perf_counter()
                y = upsample2(np, np.asarray(samples, dtype=np.float64))
                t_rs = time.perf_counter() - t
                ttfa = t_plan + t_ph + t_inf + t_rs
                rows.append(dict(rep=rep, lang=lang, text=text, chunk=first, plan_ms=t_plan * 1e3, phonemize_ms=t_ph * 1e3,
                                 inference_ms=t_inf * 1e3, resample_ms=t_rs * 1e3, ttfa_ms=ttfa * 1e3,
                                 audio_s=len(samples) / sr, rtf=t_inf / (len(samples) / sr)))
    measured = [r for r in rows if r['rep'] > 0] or rows  # first pass warms per-text caches in eSpeak
    short = [r for r in measured if len(r['text'].split()) <= 3]
    summary = dict(model=a.model, provider=a.provider, threads=a.threads, model_mb=path.stat().st_size / 2**20,
                   import_s=t_import, session_s=t_session, voices_s=t_voices, warmup_s=t_warm,
                   peak_working_set_mb=peak_mb(), n=len(measured),
                   ttfa_ms={f'p{p}': pct([r['ttfa_ms'] for r in measured], p) for p in (50, 90, 95, 99)},
                   ttfa_short_ms={f'p{p}': pct([r['ttfa_ms'] for r in short], p) for p in (50, 90, 95, 99)},
                   phonemize_ms_p50=statistics.median(r['phonemize_ms'] for r in measured),
                   inference_ms_p50=statistics.median(r['inference_ms'] for r in measured),
                   resample_ms_p50=statistics.median(r['resample_ms'] for r in measured),
                   rtf_p50=statistics.median(r['rtf'] for r in measured))
    print(json.dumps(summary, indent=1))
    if a.out:
        a.out.write_text(json.dumps(dict(summary=summary, rows=rows), ensure_ascii=False, indent=1), encoding='utf-8')


if __name__ == '__main__':
    main()
