import unittest

from omni.voice_frontend import (
    Language,
    TokenKind,
    expand_number,
    expand_version,
    normalize_for_speech,
)


class VoiceFrontendTests(unittest.TestCase):
    def test_french_number_rules_cover_uefi_values(self):
        self.assertEqual(expand_number("71", Language.FR), "soixante et onze")
        self.assertEqual(expand_number("80", Language.FR), "quatre-vingts")
        self.assertEqual(
            expand_number("8192", Language.FR),
            "huit mille cent quatre-vingt-douze",
        )

    def test_english_number_rules_cover_uefi_values(self):
        self.assertEqual(
            expand_number("8192", Language.EN),
            "eight thousand one hundred ninety-two",
        )

    def test_firmware_acronyms_are_renderer_neutral(self):
        tokens = normalize_for_speech("USB AHCI EFI NVRAM", Language.FR)
        self.assertEqual([t.kind for t in tokens], [TokenKind.ACRONYM] * 4)
        self.assertEqual(
            [t.text for t in tokens],
            ["U S B", "A H C I", "E F I", "N V R A M"],
        )

    def test_clause_semantics_are_preserved(self):
        tokens = normalize_for_speech(
            "Secure Boot désactivé. Continuer ?",
            Language.FR,
        )
        self.assertEqual(tokens[-1].kind, TokenKind.CLAUSE)
        self.assertEqual(tokens[-1].text, "question")
        self.assertIn(
            "statement",
            [t.text for t in tokens if t.kind is TokenKind.CLAUSE],
        )

    def test_invalid_number_is_rejected(self):
        with self.assertRaises(ValueError):
            expand_number("12A", Language.FR)

    def test_firmware_version_is_not_read_as_sentences(self):
        # Regression: "1.2.3" must be one VERSION token, never three digits
        # split by sentence-ending clauses.
        tokens = normalize_for_speech("BIOS 1.2.3", Language.FR)
        self.assertEqual(
            [t.kind for t in tokens], [TokenKind.ACRONYM, TokenKind.VERSION]
        )
        self.assertEqual(tokens[-1].text, "un point deux point trois")
        self.assertNotIn(TokenKind.CLAUSE, [t.kind for t in tokens])

    def test_version_components_expand_as_numbers(self):
        self.assertEqual(expand_version("2.10", Language.EN), "two point ten")
        self.assertEqual(expand_version("3.5", Language.FR), "trois point cinq")

    def test_version_stops_at_trailing_sentence_dot(self):
        # The version dots are internal; a real end-of-sentence dot is preserved.
        tokens = normalize_for_speech("Version 3.5.", Language.EN)
        self.assertEqual(tokens[-2].kind, TokenKind.VERSION)
        self.assertEqual(tokens[-1].kind, TokenKind.CLAUSE)
        self.assertEqual(tokens[-1].text, "statement")

    def test_plain_integer_and_bare_dot_are_unchanged(self):
        # Non-dotted numbers and a standalone period keep their prior behaviour.
        tokens = normalize_for_speech("Secure Boot 3.", Language.FR)
        self.assertEqual(tokens[-2].kind, TokenKind.NUMBER)
        self.assertEqual(tokens[-2].text, "trois")
        self.assertEqual(tokens[-1].kind, TokenKind.CLAUSE)

    def test_malformed_version_is_rejected(self):
        with self.assertRaises(ValueError):
            expand_version("1.", Language.FR)

    def test_large_memory_values_are_spoken_in_scale_words(self):
        # 16 MiB and 32 GiB byte counts must read as millions/milliards, not as
        # bare digit sequences.
        self.assertEqual(
            expand_number("16777216", Language.FR),
            "seize millions sept cent soixante-dix-sept mille deux cent seize",
        )
        self.assertEqual(
            expand_number("16777216", Language.EN),
            "sixteen million seven hundred seventy-seven thousand two hundred sixteen",
        )
        self.assertEqual(expand_number("1000000", Language.FR), "un million")
        self.assertEqual(expand_number("2000000", Language.FR), "deux millions")
        self.assertTrue(
            expand_number("34359738368", Language.FR).startswith("trente-quatre milliards")
        )
        self.assertTrue(
            expand_number("34359738368", Language.EN).startswith("thirty-four billion")
        )


if __name__ == "__main__":
    unittest.main()
