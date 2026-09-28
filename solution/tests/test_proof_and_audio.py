import math
import struct
import tempfile
import unittest
import wave
from pathlib import Path

from omni.audio import analyze_speech_evidence, analyze_wav, word_error_rate
from omni.proof import merkle_root, verify_merkle


class ProofAndAudioTests(unittest.TestCase):
    def test_merkle_content_and_name_binding(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            a = root / "a"
            b = root / "b"
            a.write_bytes(b"alpha")
            b.write_bytes(b"beta")
            manifest = merkle_root([b, a], base_dir=root)
            self.assertEqual(
                manifest["root"],
                merkle_root([a, b], base_dir=root)["root"],
            )
            self.assertEqual(
                verify_merkle(manifest, base_dir=root)["status"],
                "PASS",
            )

            original_root = manifest["root"]
            b.write_bytes(b"gamma")
            self.assertNotEqual(
                original_root,
                merkle_root([a, b], base_dir=root)["root"],
            )
            failed = verify_merkle(manifest, base_dir=root)
            self.assertEqual(failed["status"], "FAIL")
            self.assertIn("ARTIFACT_1_DIGEST_MISMATCH", failed["errors"])

            b.write_bytes(b"beta")
            c = root / "c"
            b.rename(c)
            renamed = merkle_root([a, c], base_dir=root)
            self.assertNotEqual(original_root, renamed["root"])

    def test_merkle_rejects_path_traversal_and_bad_digest(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            artifact = root / "ok.bin"
            artifact.write_bytes(b"known")
            manifest = merkle_root([artifact], base_dir=root)

            traversed = dict(manifest)
            traversed["artifacts"] = [dict(manifest["artifacts"][0])]
            traversed["artifacts"][0]["path"] = "../ok.bin"
            result = verify_merkle(traversed, base_dir=root)
            self.assertEqual(result["status"], "FAIL")
            self.assertIn("ARTIFACT_0_PATH_INVALID", result["errors"])

            malformed = dict(manifest)
            malformed["artifacts"] = [dict(manifest["artifacts"][0])]
            malformed["artifacts"][0]["sha256"] = "xyz"
            result = verify_merkle(malformed, base_dir=root)
            self.assertEqual(result["status"], "FAIL")
            self.assertIn("ARTIFACT_0_DIGEST_INVALID", result["errors"])

    def test_merkle_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as td, tempfile.TemporaryDirectory() as outside_td:
            root = Path(td)
            outside = Path(outside_td) / "outside.bin"
            outside.write_bytes(b"secret")
            link = root / "link.bin"
            try:
                link.symlink_to(outside)
            except OSError:
                self.skipTest("symlinks unavailable")
            with self.assertRaises(ValueError):
                merkle_root([link], base_dir=root)

    def _make_audio(self, root: Path) -> tuple[Path, Path]:
        good = root / "good.wav"
        bad = root / "bad.wav"
        rate = 24000
        samples = [
            int(9000 * math.sin(2 * math.pi * 440 * index / rate))
            for index in range(rate // 4)
        ]
        with wave.open(str(good), "wb") as stream:
            stream.setnchannels(1)
            stream.setsampwidth(2)
            stream.setframerate(rate)
            stream.writeframes(struct.pack("<" + "h" * len(samples), *samples))
        with wave.open(str(bad), "wb") as stream:
            stream.setnchannels(1)
            stream.setsampwidth(2)
            stream.setframerate(rate)
            stream.writeframes(b"\0\0" * len(samples))
        return good, bad

    def test_audio_signal_gate_does_not_claim_speech(self):
        with tempfile.TemporaryDirectory() as td:
            good, bad = self._make_audio(Path(td))
            tone = analyze_wav(good)
            self.assertEqual(tone["status"], "PASS")
            self.assertEqual(tone["scope"], "SIGNAL_INTEGRITY_ONLY")
            self.assertFalse(tone["speech_verified"])
            self.assertEqual(len(tone["sha256"]), 64)
            self.assertEqual(analyze_wav(bad)["status"], "FAIL")

    def test_speech_gate_requires_independent_transcript_match(self):
        with tempfile.TemporaryDirectory() as td:
            good, _ = self._make_audio(Path(td))
            speech = analyze_speech_evidence(
                good,
                reference_text="Secure Boot désactivé",
                recognized_text="secure boot désactivé",
                recognizer="independent-asr-test",
            )
            self.assertEqual(speech["status"], "PASS")
            self.assertTrue(speech["speech_verified"])
            self.assertEqual(speech["word_error_rate"], 0.0)

            beep_not_speech = analyze_speech_evidence(
                good,
                reference_text="Secure Boot désactivé",
                recognized_text="bip",
                recognizer="independent-asr-test",
            )
            self.assertEqual(beep_not_speech["status"], "FAIL")
            self.assertIn(
                "WORD_ERROR_RATE_EXCEEDED",
                beep_not_speech["failures"],
            )

            missing_recognizer = analyze_speech_evidence(
                good,
                reference_text="Secure Boot désactivé",
                recognized_text="Secure Boot désactivé",
                recognizer="",
            )
            self.assertEqual(missing_recognizer["status"], "FAIL")
            self.assertIn(
                "MISSING_RECOGNIZER_ID",
                missing_recognizer["failures"],
            )

    def test_word_error_rate(self):
        self.assertEqual(
            word_error_rate("Bonjour le monde", "bonjour le monde"),
            0.0,
        )
        self.assertAlmostEqual(
            word_error_rate(
                "un deux trois quatre",
                "un deux quatre",
            ),
            0.25,
        )


if __name__ == "__main__":
    unittest.main()
