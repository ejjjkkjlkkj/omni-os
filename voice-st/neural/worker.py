"""STN1 worker: persistent CPU model, no downloads, bounded binary PCM frames.

Input: one UTF-8 JSON request per line. Output: <4sIII> magic, kind, rate, bytes.
Kinds: PCM=0, DONE=1, ERROR=2, READY=3. PCM is little-endian f32 mono 48 kHz.
Text is rendered sentence by sentence, and the first sentence is split at a
clause boundary, so the first frame arrives after a short phrase rather than
after the whole utterance.
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

# Phrase-initial function words: a split just before one is a natural prosodic boundary.
FUNCTION_WORDS = set('''et ou mais donc car dans pour avec sans sur sous par vers chez entre
depuis pendant avant après qui que dont où quand si comme de du des en au aux à
and or but so because in on at to for with without from into over under by
that which who when where if than of while after before'''.split())
LEAD_GAP = {',': 0.07, ';': 0.10, ':': 0.10}

def lead_split(sentence):
    """Split the first sentence of an utterance so audio starts sooner.

    Only at a clause mark (at least 3 words after) or, in non-question sentences
    of more than 9 words, just before a function word 3..7 words in. Returns
    [(text, gap_seconds)] with one or two items.
    """
    words = sentence.split()
    if len(words) >= 4:
        for i, w in enumerate(words[:-3]):
            if w[-1] in LEAD_GAP:
                return [(' '.join(words[:i + 1]), LEAD_GAP[w[-1]]), (' '.join(words[i + 1:]), None)]
    # Questions keep one contour: a declarative-sounding head would mislead the listener.
    if len(words) > 9 and not sentence.rstrip().endswith('?'):
        for i in range(3, 8):
            if words[i].lower() in FUNCTION_WORDS:
                return [(' '.join(words[:i]), 0.02), (' '.join(words[i:]), None)]
    return [(sentence, None)]

UI_WORDS = 10

def ui_split(sentence):
    """Short announcements ("Enregistrer, bouton, indisponible.") are split at every
    comma: role/state chunks then come from the cache and only the name is new."""
    parts = [p.strip() for p in sentence.split(',')]
    if len(parts) < 2 or not all(parts):
        return None
    return [(p + ',', LEAD_GAP[',']) for p in parts[:-1]] + [(parts[-1], None)]

def plan(text):
    """[(chunk, gap after it)]: UI-style comma split for short sentences, lead
    split for the first long sentence, gaps from punctuation."""
    items = []
    for part in chunks(text):
        short = len(part.split()) <= UI_WORDS and ui_split(part)
        items.extend(short or (lead_split(part) if not items else [(part, None)]))
    return [(t, PAUSE.get(t[-1], 0.10) if g is None else g) for t, g in items]

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

def upsample2(np, x):
    """24->48 kHz: zero-stuff + 129-tap Kaiser(8.6) half-band FIR, images < -75 dB."""
    n = np.arange(-64, 65)
    h = np.sinc(n / 2) * np.kaiser(len(n), 8.6)
    h *= 2 / h.sum()
    z = np.zeros(len(x) * 2)
    z[::2] = x
    return np.convolve(z, h, mode='same')

class Cache:
    """Two-tier LRU of rendered chunks (memory, then disk across sessions).

    Screen readers repeat short UI strings constantly ("bouton.", "Menu Fichier."),
    so a hit turns hundreds of ms into ~0. The key covers everything that changes
    the audio: model digest, voice, language, speed and chunk text. The worker is
    single-threaded, so no locking is needed; disk writes are atomic (rename).
    """
    def __init__(self, np, limit_bytes, disk_dir=None, disk_limit_bytes=0, model_id=''):
        from collections import OrderedDict
        self.np, self.items, self.size, self.limit = np, OrderedDict(), 0, limit_bytes
        self.disk, self.disk_limit, self.model_id = disk_dir, disk_limit_bytes, model_id
        self.stats = dict(memory_hits=0, disk_hits=0, misses=0, entries=0, bytes=0)
        self.writes = 0
        if self.disk:
            try:
                self.disk.mkdir(parents=True, exist_ok=True)
            except OSError:
                self.disk = None
    def _file(self, key):
        digest = hashlib.sha256(json.dumps([self.model_id, *key], ensure_ascii=False).encode()).hexdigest()
        return self.disk / (digest + '.f32')
    def get(self, key):
        y = self.items.get(key)
        if y is not None:
            self.items.move_to_end(key)
            self.stats['memory_hits'] += 1
            return y
        if self.disk:
            f = self._file(key)
            try:
                y = self.np.frombuffer(f.read_bytes(), dtype='<f4')
            except OSError:
                y = None
            if y is not None and len(y):
                self.stats['disk_hits'] += 1
                self._remember(key, y)
                try:
                    os.utime(f)  # LRU order for pruning
                except OSError:
                    pass
                return y
        self.stats['misses'] += 1
        return None
    def contains(self, key):
        return key in self.items or bool(self.disk and self._file(key).exists())
    def _remember(self, key, y):
        if y.nbytes > self.limit // 4:
            return  # long prose is rarely repeated; keep room for UI strings
        self.items[key] = y
        self.size += y.nbytes
        while self.size > self.limit:
            _, old = self.items.popitem(last=False)
            self.size -= old.nbytes
        self.stats.update(entries=len(self.items), bytes=self.size)
    def put(self, key, y):
        self._remember(key, y)
        if self.disk and y.nbytes <= self.disk_limit // 64:
            f = self._file(key)
            try:
                tmp = f.with_suffix('.tmp')
                tmp.write_bytes(y.tobytes())
                os.replace(tmp, f)
            except OSError:
                return
            self.writes += 1
            if self.writes % 50 == 1:
                self._prune()
    def _prune(self):
        try:
            files = sorted(self.disk.glob('*.f32'), key=lambda p: p.stat().st_mtime)
            total = sum(p.stat().st_size for p in files)
            for p in files:
                if total <= self.disk_limit:
                    break
                total -= p.stat().st_size
                p.unlink(missing_ok=True)
        except OSError:
            pass

def render(np, model, text, voice, speed, lang):
    """One chunk -> little-endian f32 at 48 kHz, DC-free, faded edges."""
    samples, rate = model.create(text, voice=voice, speed=speed, lang=lang)
    x = np.asarray(samples, dtype=np.float64)
    if rate != 24000 or not len(x) or not np.isfinite(x).all():
        raise ValueError('Invalid native model audio')
    # DC removal and anti-imaging filter before 24->48 kHz conversion.
    x = x - x.mean()
    y = upsample2(np, x)
    peak = float(np.max(np.abs(y)))
    if not np.isfinite(peak) or peak == 0:
        raise ValueError('Silent/invalid model output')
    y *= min(1.0, 0.89 / peak)  # attenuation only; do not amplify quiet phrases
    fade = min(240, len(y) // 2)
    if fade:
        ramp = np.linspace(0, 1, fade)
        y[:fade] *= ramp
        y[-fade:] *= ramp[::-1]
    return y.astype('<f4')

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
        model_id = next(i['sha256'] for i in manifest['files'] if i['name'] == 'kokoro-v1.0.onnx')[:16]
        disk = os.environ.get('ST_CACHE_DIR') or (os.environ.get('LOCALAPPDATA') and str(Path(os.environ['LOCALAPPDATA']) / 'ST' / 'cache'))
        cache = Cache(np, int(os.environ.get('ST_NEURAL_CACHE_MB', '64')) * 2**20,
                      Path(disk) / model_id if disk and os.environ.get('ST_DISK_CACHE_MB') != '0' else None,
                      int(os.environ.get('ST_DISK_CACHE_MB', '256')) * 2**20, model_id)
        try:
            prewarm = json.loads((home / 'prewarm.json').read_text(encoding='utf-8'))
        except (OSError, ValueError):
            prewarm = {}
        frame(3)
    except Exception as e:
        frame(2, str(e).encode('utf-8'))
        return 1
    # stdin is read on a thread so a {"cancel": id} line is seen while rendering.
    import queue, threading
    lines = queue.Queue()
    def reader():
        for line in sys.stdin.buffer:
            lines.put(line)
        lines.put(None)
    threading.Thread(target=reader, daemon=True).start()
    cancelled = set()
    def is_cancelled(rid):
        while True:
            try:
                line = lines.get_nowait()
            except queue.Empty:
                return rid in cancelled
            if line is None:
                lines.put(None)
                return True
            message = json.loads(line)
            if 'cancel' not in message:
                raise ValueError('Request received before previous one finished')
            cancelled.add(message['cancel'])
    # Idle-time prewarm: after IDLE_S without requests, render frequent UI terms for
    # the last used voice, one short chunk at a time (a request waits at most one
    # chunk). Results go to the disk cache, so this happens once per voice/model.
    IDLE_S = float(os.environ.get('ST_PREWARM_IDLE_S', '2.0'))
    last = None
    pending_warm = []
    def next_line():
        idle = False  # once idle, keep warming back-to-back until a request arrives
        while True:
            try:
                return lines.get(timeout=(0.001 if idle else IDLE_S) if pending_warm else None)
            except queue.Empty:
                idle = True
                key = pending_warm.pop()
                if not cache.contains(key):
                    try:
                        cache.put(key, render(np, model, *key))
                    except Exception:
                        pass
    while (line := next_line()) is not None:
        try:
            request = json.loads(line)
            if 'cancel' in request:
                continue  # stale: that request already finished
            rid = request.get('id')
            cancelled.clear()
            text = request['text']
            if not isinstance(text, str) or len(text.encode('utf-8')) > 65536:
                raise ValueError('Invalid text size')
            speed = float(request['speed'])
            if last != (request['voice'], speed, request['lang']):
                last = (request['voice'], speed, request['lang'])
                pending_warm = [(t, *last) for t in reversed(prewarm.get(request['lang'], []))]
            parts = plan(text)
            if not parts:
                raise ValueError('No speakable text')
            for i, (part, gap) in enumerate(parts):
                if is_cancelled(rid):
                    break
                key = (part, request['voice'], speed, request['lang'])
                y = cache.get(key)
                if y is None:
                    y = render(np, model, *key)
                    cache.put(key, y)
                if i + 1 < len(parts):
                    y = np.concatenate([y, np.zeros(int(RATE * gap / speed), dtype='<f4')])
                for j in range(0, len(y), 2048):
                    if j and is_cancelled(rid):
                        break
                    frame(0, y[j:j + 2048].tobytes())
            frame(1, json.dumps(cache.stats).encode())
        except Exception as e:
            frame(2, str(e).encode('utf-8'))
    return 0

if __name__ == '__main__':
    sys.exit(main())
