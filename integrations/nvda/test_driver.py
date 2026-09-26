"""Test the NVDA synth driver outside NVDA with faithful stubs of the NVDA API it uses.

python integrations/nvda/test_driver.py <ST release dir>
Uses the real st_synth.dll (compact and, if present, neural). Exit code 0 = pass.
"""
import sys
import threading
import time
import types
from pathlib import Path

HOME = Path(sys.argv[1]).resolve()
events = []


class Notification:
    def __init__(self, name):
        self.name = name

    def notify(self, **kw):
        events.append((time.perf_counter(), self.name, kw.get("index")))


class WavePlayer:
    def __init__(self, **kw):
        assert kw["samplesPerSec"] == 48000 and kw["channels"] == 1 and kw["bitsPerSample"] == 16
        self.bytes = 0
        self.first = None
        self.stops = 0
        self.lock = threading.Lock()

    def feed(self, data, onDone=None):
        with self.lock:
            if data and self.first is None:
                self.first = time.perf_counter()
            self.bytes += len(data)
        if onDone:
            onDone()

    def idle(self):
        pass

    def stop(self):
        self.stops += 1

    def pause(self, switch):
        pass

    def close(self):
        pass


class BaseSynthDriver:
    def __init__(self):
        pass

    @staticmethod
    def VoiceSetting():
        return "voice"

    @staticmethod
    def RateSetting():
        return "rate"

    @property
    def availableVoices(self):
        return self._get_availableVoices()


class IndexCommand:
    def __init__(self, index):
        self.index = index


class BreakCommand:
    def __init__(self, time):
        self.time = time


def stub(name, **attrs):
    m = types.ModuleType(name)
    m.__dict__.update(attrs)
    sys.modules[name] = m


stub("nvwave", WavePlayer=WavePlayer, AudioPurpose=types.SimpleNamespace(SPEECH="speech"))
stub("logHandler", log=types.SimpleNamespace(error=lambda *a, **k: print("LOG ERROR", a)))
stub("speech")
stub("speech.commands", IndexCommand=IndexCommand, BreakCommand=BreakCommand)
stub("synthDriverHandler", SynthDriver=BaseSynthDriver, VoiceInfo=lambda i, n, l: (i, n, l),
     synthIndexReached=Notification("index"), synthDoneSpeaking=Notification("done"))
sys.path.insert(0, str(Path(__file__).parent / "addon" / "synthDrivers"))
import os
os.environ["ST_HOME"] = str(HOME)
import st  # noqa: E402

failures = 0


def check(cond, name):
    global failures
    print(("[PASS] " if cond else "[FAIL] ") + name)
    failures += not cond


def wait_done(timeout=20):
    end = time.time() + timeout
    while time.time() < end:
        if events and events[-1][1] == "done":
            return True
        time.sleep(0.01)
    return False


check(st.SynthDriver.check(), "driver available with this ST home")
d = st.SynthDriver()
voices = list(d.availableVoices)
print("voices:", voices)
check("fr:female" in voices, "compact voices listed")
neural = "fr:ff_siwis" in voices

for vid in (["fr:ff_siwis"] if neural else []) + ["fr:female"]:
    d._set_voice(vid)
    time.sleep(5 if vid in st.NEURAL else 0.2)  # NVDA selects the voice at startup; preload finishes before the user types
    events.clear()
    d._player.bytes = 0
    d._player.first = None
    t = time.perf_counter()
    d.speak(["Menu Fichier,", IndexCommand(1), "élément de menu.", BreakCommand(100), IndexCommand(2)])
    check(wait_done(), f"{vid}: speech done notified")
    indexes = [e[2] for e in events if e[1] == "index"]
    check(indexes == [1, 2], f"{vid}: indexes in order {indexes}")
    check(d._player.bytes > 48000, f"{vid}: audio fed from memory ({d._player.bytes} bytes)")
    print(f"       {vid}: speak -> first audio {(d._player.first - t) * 1000:.1f} ms")

    # Cancel mid-utterance: no index/done of the cancelled speech may follow.
    events.clear()
    d.speak(["Première phrase assez longue pour être interrompue.", IndexCommand(10), "Deuxième phrase.", IndexCommand(11)])
    time.sleep(0.05)
    t = time.perf_counter()
    d.cancel()
    cancel_ms = (time.perf_counter() - t) * 1000
    d.speak(["Fermer, bouton.", IndexCommand(20)])
    check(wait_done(), f"{vid}: speech after cancel completes")
    stale = [e for e in events if e[2] in (10, 11) and e[0] > t]  # reached before cancel is legitimate
    check(not stale, f"{vid}: no stale index after cancel ({stale})")
    print(f"       {vid}: cancel() returned in {cancel_ms:.2f} ms")

# Rate change without reload.
d._set_voice("fr:female")
d._set_rate(90)
events.clear()
d._player.bytes = 0
d.speak(["Bonjour, bienvenue dans ST."])
wait_done()
fast = d._player.bytes
d._set_rate(20)
events.clear()
d._player.bytes = 0
d.speak(["Bonjour, bienvenue dans ST."])
wait_done()
check(fast < d._player.bytes, f"rate 90 shorter than rate 20 ({fast} < {d._player.bytes})")

d.terminate()
print(("PASS" if not failures else "FAIL") + f": {failures} failure(s)")
sys.exit(1 if failures else 0)
