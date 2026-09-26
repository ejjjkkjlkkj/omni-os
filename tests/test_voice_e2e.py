"""End-to-end Windows speech: focus event -> semantic node -> announcement -> ST ->
PCM callback -> waveOut (real device), then a new focus cancels and replaces it.

Runs on Windows with an audio device; the ST part needs OMNI_ST_HOME (or
OMNI_ST_LIB). Timings are printed so runs leave evidence in CI logs.
"""
import math
import os
import sys
import threading
import time
import unittest

ON_WINDOWS = sys.platform == "win32"
HAVE_ST = bool(os.environ.get("OMNI_ST_HOME") or os.environ.get("OMNI_ST_LIB"))


def _device_available():
    if not ON_WINDOWS:
        return False
    try:
        from omni.audio_out import WaveOutSink
        WaveOutSink().close()
        return True
    except OSError:
        return False


HAVE_DEVICE = _device_available()


@unittest.skipUnless(HAVE_DEVICE, "needs a Windows waveOut device")
class WaveOutSinkTests(unittest.TestCase):
    def test_plays_from_memory_and_stops_immediately(self):
        from omni.audio_out import WaveOutSink
        tone = [0.05 * math.sin(2 * math.pi * 440 * n / 48000) for n in range(2048)]
        with WaveOutSink(max_queued_ms=250) as sink:
            for _ in range(10):  # ~430 ms of audio, backpressured at 250 ms
                self.assertTrue(sink(tone))
            self.assertGreater(sink.queued_ms(), 0)
            t = time.perf_counter()
            sink.stop()
            stop_ms = (time.perf_counter() - t) * 1000
            self.assertEqual(sink.queued_ms(), 0)
            print(f"\n  waveOut stop -> silence: {stop_ms:.2f} ms")
            self.assertLess(stop_ms, 50)
            self.assertTrue(sink(tone))  # device usable after stop
            self.assertTrue(sink.drain(2))

    def test_chunk_waiting_for_room_is_dropped_by_stop(self):
        from omni.audio_out import WaveOutSink
        tone = [0.05] * 4800
        with WaveOutSink(max_queued_ms=100) as sink:
            sink(tone)
            sink(tone)  # queue full now
            result = []
            t = threading.Thread(target=lambda: result.append(sink(tone)))
            t.start()
            time.sleep(0.02)
            sink.stop()
            t.join(2)
            self.assertEqual(result, [False])


@unittest.skipUnless(HAVE_DEVICE and HAVE_ST, "needs a Windows audio device and an ST release")
class EndToEndSpeechTests(unittest.TestCase):
    def test_focus_change_cancels_and_replaces_announcement(self):
        from omni.announce import SpeechController
        from omni.audio_out import WaveOutSink
        from omni.voice_st import StRenderer

        backend = os.environ.get("OMNI_ST_TEST_BACKEND", "neural")
        log = []  # (time, utterance-in-progress) for every chunk the device accepted

        with StRenderer(lang="fr", backend=backend) as renderer, WaveOutSink() as device:
            class Probe:
                current = None

                def __call__(self, chunk):
                    ok = device(chunk)
                    if ok:
                        log.append((time.perf_counter(), Probe.current))
                    return ok

                def stop(self):
                    device.stop()

            class TaggingRenderer:
                def speak(self, text, sink):
                    Probe.current = text
                    return renderer.speak(text, sink)

                def cancel(self):
                    renderer.cancel()

            c = SpeechController(TaggingRenderer(), Probe(), "fr")
            events = [
                {"sequence": 0, "kind": "node_created", "node": {"id": 1, "role": "menuitem", "name": "Fichier"}},
                {"sequence": 1, "kind": "node_created", "node": {"id": 2, "role": "checkbox", "name": "Démarrage sécurisé", "state": ["checked"]}},
                {"sequence": 2, "kind": "node_created", "node": {"id": 3, "role": "edit", "name": "Mot de passe", "state": ["password"], "value": "hunter2"}},
            ]
            c.model.apply_all(events)
            long_name = "Rapport annuel détaillé de l'exercice précédent, avec annexes et tableaux complémentaires"
            c.apply({"sequence": 3, "kind": "node_updated", "node": {"id": 1, "name": long_name}})
            t_focus1 = time.perf_counter()
            c.apply({"sequence": 4, "kind": "focus_changed", "node": {"id": 1}})
            while not log and time.perf_counter() - t_focus1 < 10:
                time.sleep(0.002)
            self.assertTrue(log, "first announcement produced no audio")
            first_audio_ms = (log[0][0] - t_focus1) * 1000
            time.sleep(0.15)  # user hears the start, then presses Tab
            t_focus2 = time.perf_counter()
            c.apply({"sequence": 5, "kind": "focus_changed", "node": {"id": 2}})
            cancel_ms = (time.perf_counter() - t_focus2) * 1000
            self.assertEqual(device.queued_ms(), 0)  # old announcement silenced
            t_focus3 = None
            while time.perf_counter() - t_focus2 < 10 and not any(t > t_focus2 and u.startswith("Démarrage") for t, u in log):
                time.sleep(0.002)
            second_audio_ms = (min(t for t, u in log if t > t_focus2) - t_focus2) * 1000
            time.sleep(0.3)
            t_focus3 = time.perf_counter()
            c.apply({"sequence": 6, "kind": "focus_changed", "node": {"id": 3}})
            self.assertTrue(c.wait_idle(30))
            device.drain(10)
            c.close()

        after2 = [u for t, u in log if t > t_focus2 + 0.001]
        after3 = [u for t, u in log if t > t_focus3 + 0.001]
        self.assertTrue(after2, "second announcement produced no audio")
        self.assertFalse(any(u.startswith("Rapport") for u in after2), "stale announcement kept playing")
        self.assertFalse(any(u.startswith("Démarrage") for u in after3), "stale announcement kept playing")
        self.assertEqual(c.spoken[-1], "Mot de passe, zone d'édition, protégé, 7 caractères.")
        self.assertTrue(all("hunter2" not in u for _, u in log))
        print(f"\n  [{backend}] focus->audio {first_audio_ms:.1f} ms | focus change->cancel returned {cancel_ms:.2f} ms"
              f" | new focus->audio {second_audio_ms:.1f} ms | chunks {len(log)}")


if __name__ == "__main__":
    unittest.main()
