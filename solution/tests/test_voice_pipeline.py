from pathlib import Path
import unittest

from omni.voice_frontend import Language
from omni.voice_phonemes import FRONTEND_STREAM_VERSION, PhonemeId, validate_frontend_stream
from omni.voice_pipeline import MAX_STREAM_BYTES, MAX_TEXT_CHARS, compile_speech_stream
from omni.voice_quality import PCM_ALLOWED_CHANNELS, PCM_SAMPLE_RATE


class VoicePipelineTests(unittest.TestCase):
    def test_french_uefi_phrase_compiles_to_valid_stream(self):
        stream = compile_speech_stream(
            "USB 8192. Secure Boot désactivé. Continuer ?",
            Language.FR,
        )
        self.assertTrue(validate_frontend_stream(stream))
        self.assertIn(int(PhonemeId.WORD_BOUNDARY), stream)
        self.assertEqual(stream[-2:], bytes([PhonemeId.PAUSE, PhonemeId.CLAUSE_END]))

    def test_pipeline_is_deterministic(self):
        text = "NVRAM 1280 x 800"
        self.assertEqual(
            compile_speech_stream(text, "fr"),
            compile_speech_stream(text, "fr"),
        )

    def test_acronym_path_keeps_letter_spelling(self):
        stream = compile_speech_stream("USB", "en")
        self.assertEqual(
            stream,
            bytes([22, 10, 54, 5, 25, 54, 35, 8, 54]),
        )

    def test_firmware_input_is_bounded(self):
        with self.assertRaises(ValueError):
            compile_speech_stream("a" * (MAX_TEXT_CHARS + 1), "fr")

    def test_c_header_matches_python_voice_contract(self):
        header = Path("include/omni_voice_frontend.h").read_text(encoding="utf-8")
        expected = {
            "OMNI_VOICE_FRONTEND_STREAM_VERSION": FRONTEND_STREAM_VERSION,
            "OMNI_VOICE_MAX_TEXT_CHARS": MAX_TEXT_CHARS,
            "OMNI_VOICE_MAX_STREAM_BYTES": MAX_STREAM_BYTES,
            "OMNI_VOICE_PCM_SAMPLE_RATE": PCM_SAMPLE_RATE,
            "OMNI_VOICE_PCM_MAX_CHANNELS": max(PCM_ALLOWED_CHANNELS),
        }
        for name, value in expected.items():
            self.assertIn(f"#define {name} UINT32_C({value})", header)


if __name__ == "__main__":
    unittest.main()
