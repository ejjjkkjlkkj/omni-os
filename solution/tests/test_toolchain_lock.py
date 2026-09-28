import json
import shutil
import tempfile
import unittest
from pathlib import Path

from tools.verify_toolchain_lock import ROOT, verify


class ToolchainLockTests(unittest.TestCase):
    def test_external_verification_inputs_match_lock(self):
        result = verify()
        self.assertEqual(result["status"], "PASS", result)
        self.assertGreater(len(result["checked_files"]), 0)

    def test_qemu_tarball_hash_drift_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "repo"
            shutil.copytree(
                ROOT,
                work,
                ignore=shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache"),
            )
            lock = json.loads((work / "toolchains.lock.json").read_text(encoding="utf-8"))
            expected = lock["qemu"]["tarball_sha256"]
            script = work / "scripts" / "build_pinned_qemu.sh"
            text = script.read_text(encoding="utf-8")
            self.assertIn(expected, text)
            script.write_text(text.replace(expected, "0" * 64), encoding="utf-8")

            result = verify(work)
            self.assertEqual(result["status"], "FAIL")
            self.assertTrue(
                any(
                    item["file"] == "scripts/build_pinned_qemu.sh"
                    and expected in item["value"]
                    for item in result["missing"]
                ),
                result,
            )

    def test_invalid_hash_in_lock_fails_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "repo"
            shutil.copytree(
                ROOT,
                work,
                ignore=shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache"),
            )
            lock_path = work / "toolchains.lock.json"
            lock = json.loads(lock_path.read_text(encoding="utf-8"))
            lock["qemu"]["tarball_sha256"] = "not-a-sha256"
            lock_path.write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")

            result = verify(work)
            self.assertEqual(result["status"], "FAIL")
            self.assertTrue(result["invalid"], result)


    def test_comment_cannot_mask_qemu_hash_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "repo"
            shutil.copytree(
                ROOT,
                work,
                ignore=shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache"),
            )
            lock = json.loads((work / "toolchains.lock.json").read_text(encoding="utf-8"))
            expected = lock["qemu"]["tarball_sha256"]
            script = work / "scripts" / "build_pinned_qemu.sh"
            text = script.read_text(encoding="utf-8")
            replacement = 'QEMU_TARBALL_SHA256="' + ("0" * 64) + '"'
            text = text.replace(f'QEMU_TARBALL_SHA256="{expected}"', replacement)
            text += f"\n# stale expected hash must not satisfy the verifier: {expected}\n"
            script.write_text(text, encoding="utf-8")

            result = verify(work)
            self.assertEqual(result["status"], "FAIL")
            self.assertTrue(
                any(
                    item["file"] == "scripts/build_pinned_qemu.sh"
                    and "QEMU_TARBALL_SHA256" in item["value"]
                    for item in result["missing"]
                ),
                result,
            )

    def test_unrelated_value_cannot_mask_contextual_edk2_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "repo"
            shutil.copytree(
                ROOT,
                work,
                ignore=shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache"),
            )
            lock = json.loads((work / "toolchains.lock.json").read_text(encoding="utf-8"))
            expected = lock["edk2_primary"]["commit"]
            workflow = work / ".github" / "workflows" / "reproducibility.yml"
            text = workflow.read_text(encoding="utf-8")
            text = text.replace(f"EDK2_SHA: {expected}", "EDK2_SHA: " + ("0" * 40))
            text += f"\n# stale expected commit must not satisfy the verifier: {expected}\n"
            workflow.write_text(text, encoding="utf-8")

            result = verify(work)
            self.assertEqual(result["status"], "FAIL")
            self.assertTrue(
                any(
                    item["file"] == ".github/workflows/reproducibility.yml"
                    and item["value"] == f"EDK2_SHA: {expected}"
                    for item in result["missing"]
                ),
                result,
            )


    def test_unsupported_qemu_slirp_mode_fails_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "repo"
            shutil.copytree(
                ROOT,
                work,
                ignore=shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache"),
            )
            lock_path = work / "toolchains.lock.json"
            lock = json.loads(lock_path.read_text(encoding="utf-8"))
            lock["qemu"]["slirp_mode"] = "auto"
            lock_path.write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")

            result = verify(work)
            self.assertEqual(result["status"], "FAIL")
            self.assertTrue(
                any(
                    item["field"] == "qemu.slirp_mode"
                    for item in result["invalid"]
                ),
                result,
            )


if __name__ == "__main__":
    unittest.main()
