import json
import os
import threading
import time
import unittest

from omni import voice_st
from omni.voice_frontend import Language, normalize_for_speech
from omni.voice_quality import inspect_pcm16le

ST_AVAILABLE = bool(os.environ.get("OMNI_ST_LIB") or os.environ.get("OMNI_ST_HOME"))


class StRendererContractTests(unittest.TestCase):
    """Pure contract tests: run everywhere, no ST binary needed."""

    def test_frontend_tokens_become_punctuated_text(self):
        tokens = normalize_for_speech("USB 8192 ?", Language.FR)
        text = voice_st.tokens_to_text(tokens)
        self.assertTrue(text.startswith("U S B "))
        self.assertTrue(text.endswith("?"))
        self.assertNotIn("8192", text)

    def test_config_json_is_strict(self):
        cfg = json.loads(voice_st.config_json(lang="fr", backend="neural", voice="ff_siwis", rate=120))
        self.assertEqual(cfg, {"backend": "neural", "lang": "fr", "voice": "ff_siwis", "rate": 120})
        self.assertNotIn("voice", json.loads(voice_st.config_json(lang="en", backend="compact")))
        with self.assertRaises(ValueError):
            voice_st.config_json(lang="fr", backend="sapi")
        with self.assertRaises(ValueError):
            voice_st.config_json(lang="de")
        with self.assertRaises(ValueError):
            voice_st.config_json(lang="fr", rate=10)

    def test_float_conversion_never_hits_clipping_codes(self):
        pcm = voice_st.float_to_pcm16le([0.0, 0.5, -0.5, 1.0, -1.0, 2.0])
        values = [int.from_bytes(pcm[i:i + 2], "little", signed=True) for i in range(0, len(pcm), 2)]
        self.assertEqual(values, [0, 16384, -16384, 32766, -32766, 32766])
        self.assertEqual(inspect_pcm16le(pcm * 2000).clipped_samples, 0)

    def test_wav_header_matches_voicecore_contract(self):
        pcm = voice_st.float_to_pcm16le([0.1] * 480)
        wav = voice_st.pcm16le_wav(pcm)
        self.assertEqual(wav[:4], b"RIFF")
        self.assertEqual(int.from_bytes(wav[24:28], "little"), 48000)
        self.assertEqual(int.from_bytes(wav[34:36], "little"), 16)
        self.assertEqual(wav[44:], pcm)
        with self.assertRaises(ValueError):
            voice_st.pcm16le_wav(b"\x00")

    def test_missing_library_is_explicit(self):
        saved = {k: os.environ.pop(k, None) for k in ("OMNI_ST_LIB", "OMNI_ST_HOME")}
        try:
            with self.assertRaises(FileNotFoundError):
                voice_st.find_library()
        finally:
            for k, v in saved.items():
                if v is not None:
                    os.environ[k] = v


@unittest.skipUnless(ST_AVAILABLE, "set OMNI_ST_HOME (or OMNI_ST_LIB) to an ST 0.6+ release")
class StRendererIntegrationTests(unittest.TestCase):
    """Real ST library through the C ABI v1."""

    def test_compact_render_passes_pcm_gate(self):
        with voice_st.StRenderer(lang="fr", backend="compact", voice="female") as r:
            result = r.render("Menu Fichier, 3 éléments.")
        self.assertTrue(result.report.clean, result.report.violations)
        self.assertGreater(result.seconds, 0.5)
        self.assertIn("trois", result.text)

    def test_neural_render_cancel_and_reuse(self):
        backend = os.environ.get("OMNI_ST_TEST_BACKEND", "neural")
        with voice_st.StRenderer(lang="fr", backend=backend) as r:
            first = r.render("Bonjour, bienvenue dans ST.")
            self.assertTrue(first.report.clean, first.report.violations)
            long_text = " ".join(["Cette phrase sera interrompue."] * 8)
            timer = threading.Timer(0.05, r.cancel)
            timer.start()
            with self.assertRaises(voice_st.StCancelled):
                r.speak(long_text, lambda chunk: time.sleep(0.01))
            timer.join()
            again = r.render("Fermer, bouton.")
            self.assertTrue(again.report.clean)
            self.assertLess(again.first_audio_ms, 2000)

    def test_sink_can_stop_speech(self):
        with voice_st.StRenderer(lang="en", backend="compact") as r:
            chunks = []
            with self.assertRaises(voice_st.StCancelled):
                r.speak("One. Two. Three. Four.", lambda c: chunks.append(len(c)) or len(chunks) < 2)
            self.assertEqual(len(chunks), 2)


if __name__ == "__main__":
    unittest.main()
