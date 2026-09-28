import hashlib
import tempfile
import unittest
from pathlib import Path

from tools.verify_uefi_evidence import EvidenceError, parse_evidence, verify

CHALLENGE = "a" * 64
OTHER_CHALLENGE = "b" * 64
PLATFORM_UUID = "12345678-1234-5678-9abc-def012345678"
OTHER_PLATFORM_UUID = "87654321-4321-8765-cba9-876543210fed"
VALID = f"""OMNI_EVIDENCE_V2
OMNI_CHALLENGE={CHALLENGE}
OMNI_PLATFORM_UUID={PLATFORM_UUID}
OMNI_HII_HANDLES=2
OMNI_HII_FORM_PACKAGES=3
OMNI_HII_OPCODES=21
OMNI_HII_QUESTIONS=8
OMNI_HII_PASSWORDS=1
OMNI_HII_INVALID=0
OMNI_HII_PASS
OMNI_UEFI_PASS
"""


class PhysicalEvidenceTests(unittest.TestCase):
    def test_valid_evidence_is_bound_to_efi_hash_and_challenge(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            evidence = root / "OMNI-EVIDENCE.TXT"
            efi = root / "BOOTX64.EFI"
            evidence.write_text(VALID, encoding="ascii")
            efi.write_bytes(b"known-efi")
            expected = hashlib.sha256(b"known-efi").hexdigest()
            result = verify(
                evidence,
                efi,
                expected,
                CHALLENGE,
                PLATFORM_UUID,
            )
            self.assertEqual(
                result["status"], "PHYSICAL_EVIDENCE_SOFTWARE_VERIFY_PASS"
            )
            self.assertEqual(result["challenge"], CHALLENGE)
            self.assertEqual(result["platform_uuid"], PLATFORM_UUID)

    def test_stale_challenge_is_fatal(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            evidence = root / "OMNI-EVIDENCE.TXT"
            efi = root / "BOOTX64.EFI"
            evidence.write_text(VALID, encoding="ascii")
            efi.write_bytes(b"known-efi")
            expected = hashlib.sha256(b"known-efi").hexdigest()
            with self.assertRaises(EvidenceError):
                verify(evidence, efi, expected, OTHER_CHALLENGE)

    def test_platform_uuid_mismatch_is_fatal(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            evidence = root / "OMNI-EVIDENCE.TXT"
            efi = root / "BOOTX64.EFI"
            evidence.write_text(VALID, encoding="ascii")
            efi.write_bytes(b"known-efi")
            expected = hashlib.sha256(b"known-efi").hexdigest()
            with self.assertRaises(EvidenceError):
                verify(
                    evidence,
                    efi,
                    expected,
                    CHALLENGE,
                    OTHER_PLATFORM_UUID,
                )

    def test_missing_platform_uuid_is_fatal_when_required(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            evidence = root / "OMNI-EVIDENCE.TXT"
            efi = root / "BOOTX64.EFI"
            evidence.write_text(
                VALID.replace(f"OMNI_PLATFORM_UUID={PLATFORM_UUID}\n", ""),
                encoding="ascii",
            )
            efi.write_bytes(b"known-efi")
            expected = hashlib.sha256(b"known-efi").hexdigest()
            with self.assertRaises(EvidenceError):
                verify(evidence, efi, expected, CHALLENGE, PLATFORM_UUID)

    def test_platform_uuid_is_optional_for_legacy_evidence(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            evidence = root / "OMNI-EVIDENCE.TXT"
            efi = root / "BOOTX64.EFI"
            evidence.write_text(
                VALID.replace(f"OMNI_PLATFORM_UUID={PLATFORM_UUID}\n", ""),
                encoding="ascii",
            )
            efi.write_bytes(b"known-efi")
            expected = hashlib.sha256(b"known-efi").hexdigest()
            result = verify(evidence, efi, expected, CHALLENGE)
            self.assertIsNone(result["platform_uuid"])

    def test_invalid_platform_uuid_in_evidence_is_fatal(self):
        with self.assertRaises(EvidenceError):
            parse_evidence(
                VALID.replace(
                    f"OMNI_PLATFORM_UUID={PLATFORM_UUID}",
                    "OMNI_PLATFORM_UUID=00000000-0000-0000-0000-000000000000",
                )
            )

    def test_firmware_fail_marker_is_fatal(self):
        with self.assertRaises(EvidenceError):
            parse_evidence(VALID.replace("OMNI_HII_PASS", "OMNI_HII_FAIL"))

    def test_nonzero_invalid_count_is_fatal(self):
        with self.assertRaises(EvidenceError):
            parse_evidence(VALID.replace("OMNI_HII_INVALID=0", "OMNI_HII_INVALID=1"))

    def test_wrong_binary_hash_is_fatal(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            evidence = root / "OMNI-EVIDENCE.TXT"
            efi = root / "BOOTX64.EFI"
            evidence.write_text(VALID, encoding="ascii")
            efi.write_bytes(b"tampered")
            expected = hashlib.sha256(b"known-efi").hexdigest()
            with self.assertRaises(EvidenceError):
                verify(evidence, efi, expected, CHALLENGE)


if __name__ == "__main__":
    unittest.main()
