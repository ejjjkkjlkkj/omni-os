import hashlib
import tempfile
import unittest
from pathlib import Path

from tools.prepare_physical_media import prepare
from tools.verify_uefi_evidence import EvidenceError

CHALLENGE = "c" * 64


class PreparePhysicalMediaTests(unittest.TestCase):
    def _media(self, root: Path) -> tuple[Path, str]:
        mount = root / "media"
        efi = mount / "EFI" / "BOOT" / "BOOTX64.EFI"
        efi.parent.mkdir(parents=True)
        efi.write_bytes(b"known-efi")
        return mount, hashlib.sha256(b"known-efi").hexdigest()

    def test_prepares_fresh_challenge_and_removes_stale_evidence(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            mount, expected = self._media(root)
            (mount / "OMNI-EVIDENCE.TXT").write_text("stale", encoding="ascii")
            challenge_out = root / "expected-challenge.txt"
            result = prepare(mount, expected, challenge_out, CHALLENGE)
            self.assertEqual(result["status"], "PHYSICAL_MEDIA_PREPARED")
            self.assertFalse((mount / "OMNI-EVIDENCE.TXT").exists())
            self.assertEqual(
                (mount / "OMNI-CHALLENGE.TXT").read_text(encoding="ascii").strip(),
                CHALLENGE,
            )
            self.assertEqual(challenge_out.read_text(encoding="ascii").strip(), CHALLENGE)

    def test_rejects_wrong_efi_hash(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            mount, _ = self._media(root)
            with self.assertRaises(EvidenceError):
                prepare(mount, "0" * 64, root / "challenge.txt", CHALLENGE)

    def test_rejects_expected_challenge_file_on_boot_media(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            mount, expected = self._media(root)
            with self.assertRaises(EvidenceError):
                prepare(mount, expected, mount / "expected.txt", CHALLENGE)


if __name__ == "__main__":
    unittest.main()
