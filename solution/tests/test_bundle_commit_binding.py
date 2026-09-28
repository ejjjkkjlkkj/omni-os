import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path

from tools.deterministic_bundle import create_bundle
from tools.verify_bundle_commit import verify_bundle_commit


class BundleCommitBindingTests(unittest.TestCase):
    def make_repo(self, root: Path) -> str:
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        (root / "a.txt").write_text("alpha\n", encoding="utf-8")
        (root / "nested").mkdir()
        (root / "nested" / "b.bin").write_bytes(b"\x00\x01\x02")
        run_script = root / "run.sh"
        run_script.write_text("#!/bin/sh\necho omni\n", encoding="utf-8")
        run_script.chmod(0o755)
        subprocess.run(["git", "-C", str(root), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(root), "update-index", "--chmod=+x", "run.sh"],
            check=True,
        )
        subprocess.run(
            [
                "git",
                "-C",
                str(root),
                "-c",
                "user.name=omni-test",
                "-c",
                "user.email=omni@example.invalid",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
            check=True,
        )
        return subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            text=True,
        ).strip()

    def test_clean_bundle_is_bound_to_exact_commit(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            commit = self.make_repo(root)
            bundle = root / "bundle.zip"
            create_bundle(root, bundle)

            result = verify_bundle_commit(root, bundle, commit)
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(result["commit"], commit)

    def test_executable_git_mode_is_preserved(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.make_repo(root)
            bundle = root / "bundle.zip"
            create_bundle(root, bundle)

            with zipfile.ZipFile(bundle, "r") as archive:
                mode = (archive.getinfo("run.sh").external_attr >> 16) & 0xFFFF
            self.assertEqual(mode, 0o100755)

    def test_worktree_mutation_cannot_masquerade_as_commit_bundle(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            commit = self.make_repo(root)
            (root / "a.txt").write_text("mutated but uncommitted\n", encoding="utf-8")

            bundle = root / "dirty.zip"
            with self.assertRaisesRegex(ValueError, "tracked worktree is dirty"):
                create_bundle(root, bundle)


if __name__ == "__main__":
    unittest.main()
