import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.verify_git_snapshot import verify_snapshot


class VerifyGitSnapshotTests(unittest.TestCase):
    def make_repo(self, root: Path) -> str:
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        (root / "tracked.txt").write_text("alpha\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "tracked.txt"], check=True)
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

    def test_clean_exact_commit_passes(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            commit = self.make_repo(root)
            result = verify_snapshot(root, commit)
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(result["head_commit"], commit)

    def test_wrong_commit_fails(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.make_repo(root)
            result = verify_snapshot(root, "0" * 40)
            self.assertEqual(result["status"], "FAIL")
            self.assertIn("HEAD_COMMIT_MISMATCH", result["failures"])

    def test_dirty_worktree_fails(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            commit = self.make_repo(root)
            (root / "tracked.txt").write_text("mutated\n", encoding="utf-8")
            result = verify_snapshot(root, commit)
            self.assertEqual(result["status"], "FAIL")
            self.assertIn("TRACKED_WORKTREE_DIRTY", result["failures"])

    def test_dirty_index_fails(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            commit = self.make_repo(root)
            (root / "tracked.txt").write_text("staged\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "tracked.txt"], check=True)
            result = verify_snapshot(root, commit)
            self.assertEqual(result["status"], "FAIL")
            self.assertIn("INDEX_DIRTY", result["failures"])


if __name__ == "__main__":
    unittest.main()
