"""Profile Kokoro ONNX latency: load time, first-sentence latency and RTF per thread count.

Usage: python scripts/neural_profile.py [model.onnx] [threads...]
"""
import sys, time
from pathlib import Path
import onnxruntime as rt
from kokoro_onnx import Kokoro

HOME = Path(__file__).resolve().parents[1] / 'neural' / 'models'
model_path = Path(sys.argv[1]) if len(sys.argv) > 1 else HOME / 'kokoro-v1.0.onnx'
threads = [int(x) for x in sys.argv[2:]] or [2, 4, 8]
CASES = [
    ('fr-fr', 'ff_siwis', 'Bonjour, bienvenue dans ST.'),
    ('fr-fr', 'ff_siwis', 'Menu Fichier.'),
    ('en-us', 'af_heart', 'Hello, welcome to ST. The screen reader is ready.'),
]
for n in threads:
    o = rt.SessionOptions()
    o.intra_op_num_threads = n
    o.inter_op_num_threads = 1
    o.graph_optimization_level = rt.GraphOptimizationLevel.ORT_ENABLE_ALL
    t = time.perf_counter()
    k = Kokoro.from_session(rt.InferenceSession(str(model_path), sess_options=o, providers=['CPUExecutionProvider']), str(HOME / 'voices-v1.0.bin'))
    load = time.perf_counter() - t
    k.create('Warm up.', voice='af_heart', lang='en-us')
    for lang, voice, text in CASES:
        best = 1e9
        for _ in range(3):
            t = time.perf_counter()
            s, sr = k.create(text, voice=voice, lang=lang)
            best = min(best, time.perf_counter() - t)
        dur = len(s) / sr
        print(f'{model_path.name} threads={n} load={load:.2f}s {text[:28]!r:32} synth={best*1000:.0f}ms audio={dur:.2f}s RTF={best/dur:.3f}', flush=True)
