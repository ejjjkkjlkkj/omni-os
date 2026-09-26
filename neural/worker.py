"""STN1 worker: persistent CPU model, no downloads, bounded binary PCM frames.

Input: one UTF-8 JSON request per line. Output: <4sIII> magic, kind, rate, bytes.
Kinds: PCM=0, DONE=1, ERROR=2, READY=3. PCM is little-endian f32 mono 48 kHz.
Text is rendered sentence by sentence so the first frame arrives after the
first sentence, not after the whole utterance.
"""
import argparse, hashlib, json, os, re, struct, sys
from pathlib import Path

RATE = 48000
PAUSE = {'.': 0.18, '!': 0.18, '?': 0.20, ':': 0.12, ';': 0.12, '\n': 0.22}
# Sentence end, then optional closing quote/bracket, then whitespace.
SPLIT = re.compile(r'(?<=[.!?…:;])["»”)\]]?\s+|\n+')
MAX_CHUNK = 220

def frame(kind, data=b''):
    sys.stdout.buffer.write(struct.pack('<4sIII', b'STN1', kind, RATE, len(data)))
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()

def chunks(text):
    """Sentences; overlong ones are cut at commas, then at spaces."""
    for sentence in SPLIT.split(text):
        sentence = sentence.strip()
        if not any(c.isalnum() for c in sentence):
            continue
        while len(sentence) > MAX_CHUNK:
            cut = sentence.rfind(', ', 0, MAX_CHUNK)
            if cut < MAX_CHUNK // 2:
                cut = sentence.rfind(' ', 0, MAX_CHUNK)
            if cut <= 0:
                cut = MAX_CHUNK
            yield sentence[:cut + 1].strip()
            sentence = sentence[cut + 1:].strip()
        if sentence:
            yield sentence

def verify(home, manifest):
    """Full SHA-256 once per file version; later starts check size+mtime stamp."""
    stamp_path = home / 'models' / '.verified'
    try:
        stamps = json.loads(stamp_path.read_text(encoding='utf-8'))
    except Exception:
        stamps = {}
    changed = False
    for item in manifest['files']:
        p = home / 'models' / item['name']
        st = p.stat()
        key = f"{item['sha256']}:{st.st_size}:{st.st_mtime_ns}"
        if stamps.get(item['name']) == key:
            continue
        with p.open('rb') as f:
            if hashlib.file_digest(f, 'sha256').hexdigest() != item['sha256']:
                raise ValueError('Model checksum mismatch: ' + item['name'])
        stamps[item['name']] = key
        changed = True
    if changed:
        try:
            stamp_path.write_text(json.dumps(stamps), encoding='utf-8')
        except OSError:
            pass  # read-only install: verification simply repeats next start

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--home', type=Path, required=True)
    home = parser.parse_args().home
    try:
        import numpy as np
        import onnxruntime as rt
        from scipy.signal import resample_poly
        from kokoro_onnx import Kokoro
        manifest = json.loads((home / 'models.json').read_text(encoding='utf-8-sig'))
        verify(home, manifest)
        options = rt.SessionOptions()
        # Measured on 8-core Ryzen: 4 threads is the latency knee; more adds contention.
        options.intra_op_num_threads = max(1, min(4, (os.cpu_count() or 2) // 2))
        options.inter_op_num_threads = 1
        options.graph_optimization_level = rt.GraphOptimizationLevel.ORT_ENABLE_ALL
        model = Kokoro.from_session(rt.InferenceSession(str(home / 'models/kokoro-v1.0.onnx'), sess_options=options, providers=['CPUExecutionProvider']), str(home / 'models/voices-v1.0.bin'))
        # Warm both phonemizer languages and the ONNX graph before READY.
        model.create('Bonjour.', voice='ff_siwis', lang='fr-fr')
        model.create('Hello.', voice='af_heart', lang='en-us')
        frame(3)
    except Exception as e:
        frame(2, str(e).encode('utf-8'))
        return 1
    for line in sys.stdin.buffer:
        try:
            request = json.loads(line)
            text = request['text']
            if not isinstance(text, str) or len(text.encode('utf-8')) > 65536:
                raise ValueError('Invalid text size')
            speed = float(request['speed'])
            parts = list(chunks(text))
            if not parts:
                raise ValueError('No speakable text')
            for i, part in enumerate(parts):
                samples, rate = model.create(part, voice=request['voice'], speed=speed, lang=request['lang'])
                x = np.asarray(samples, dtype=np.float64)
                if rate != 24000 or not len(x) or not np.isfinite(x).all():
                    raise ValueError('Invalid native model audio')
                # DC removal and anti-imaging filter before 24->48 kHz conversion.
                x = x - x.mean()
                y = resample_poly(x, 2, 1, window=('kaiser', 8.6))
                peak = float(np.max(np.abs(y)))
                if not np.isfinite(peak) or peak == 0:
                    raise ValueError('Silent/invalid model output')
                y *= min(1.0, 0.89 / peak)  # attenuation only; do not amplify quiet phrases
                fade = min(240, len(y) // 2)
                if fade:
                    ramp = np.linspace(0, 1, fade)
                    y[:fade] *= ramp
                    y[-fade:] *= ramp[::-1]
                if i + 1 < len(parts):
                    gap = PAUSE.get(part[-1], 0.10) / speed
                    y = np.concatenate([y, np.zeros(int(RATE * gap))])
                y = y.astype('<f4')
                for j in range(0, len(y), 2048):
                    frame(0, y[j:j + 2048].tobytes())
            frame(1)
        except Exception as e:
            frame(2, str(e).encode('utf-8'))
    return 0

if __name__ == '__main__':
    sys.exit(main())
