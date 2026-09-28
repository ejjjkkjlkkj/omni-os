import unittest

from omni.voice_phonemes import (
    FRONTEND_STREAM_VERSION,
    PhonemeId,
    frontend_phonemes,
    validate_frontend_stream,
)


class VoiceFrontendTests(unittest.TestCase):
    def test_stream_contract_is_stable(self):
        self.assertEqual(FRONTEND_STREAM_VERSION, 1)
        self.assertEqual(int(PhonemeId.WORD_BOUNDARY), 52)
        self.assertEqual(int(PhonemeId.CLAUSE_END), 53)
        self.assertEqual(int(PhonemeId.PAUSE), 54)

    def test_french_frontend_is_deterministic(self):
        expected = bytes([35, 48, 28, 10, 20])
        self.assertEqual(frontend_phonemes("bonjour", "fr"), expected)
        self.assertEqual(frontend_phonemes("bonjour", "fr"), expected)

    def test_english_acronym_spells_letters(self):
        self.assertEqual(
            frontend_phonemes("USB", "en"),
            bytes([22, 10, 54, 5, 25, 54, 35, 8, 54]),
        )

    def test_numbers_and_clause_markers(self):
        fr = frontend_phonemes("128.", "fr")
        self.assertTrue(validate_frontend_stream(fr))
        self.assertEqual(fr[-2:], bytes([PhonemeId.PAUSE, PhonemeId.CLAUSE_END]))

    def test_word_boundary_is_explicit(self):
        stream = frontend_phonemes("Secure Boot", "en")
        self.assertIn(int(PhonemeId.WORD_BOUNDARY), stream)

    def test_invalid_language_is_rejected(self):
        with self.assertRaises(ValueError):
            frontend_phonemes("bonjour", "de")

    def test_invalid_stream_is_rejected(self):
        self.assertTrue(validate_frontend_stream(bytes([1, 52, 54])))
        self.assertFalse(validate_frontend_stream(bytes([0, 1])))
        self.assertFalse(validate_frontend_stream(bytes([55])))


if __name__ == "__main__":
    unittest.main()
