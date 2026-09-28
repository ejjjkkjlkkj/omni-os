import hashlib
import json
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path

from tools.build_provenance import build_statement
from tools.deterministic_bundle import MANIFEST_NAME, create_bundle


class DeterministicBundleTests(unittest.TestCase):
    def make_repo(self, root: Path) -> None:
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        (root / "a.txt").write_text("alpha\n", encoding="utf-8")
        (root / "nested").mkdir()
        (root / "nested" / "b.bin").write_bytes(b"\x00\x01\x02")
        subprocess.run(["git", "-C", str(root), "add", "a.txt", "nested/b.bin"], check=True)
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

    def test_bundle_ignores_untracked_noise_and_binds_committed_content(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.make_repo(root)

            first = root / "first.zip"
            second = root / "second.zip"
            digest1 = create_bundle(root, first)

            (root / "untracked-noise.txt").write_text("must not enter proof\n", encoding="utf-8")
            digest2 = create_bundle(root, second)

            self.assertEqual(digest1, digest2)
            self.assertEqual(first.read_bytes(), second.read_bytes())

            with zipfile.ZipFile(first) as archive:
                self.assertEqual(
                    set(archive.namelist()),
                    {"a.txt", "nested/b.bin", MANIFEST_NAME},
                )
                manifest = json.loads(archive.read(MANIFEST_NAME))
                self.assertEqual(manifest["schema"], "omni.bundle-manifest.v1")
                self.assertEqual(
                    [entry["path"] for entry in manifest["files"]],
                    ["a.txt", "nested/b.bin"],
                )

            (root / "a.txt").write_text("changed\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "a.txt"], check=True)
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
                    "change tracked content",
                ],
                check=True,
            )
            changed = root / "changed.zip"
            digest3 = create_bundle(root, changed)
            self.assertNotEqual(digest1, digest3)

    def test_bundle_rejects_dirty_index(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.make_repo(root)
            (root / "staged.txt").write_text("not committed\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "staged.txt"], check=True)

            with self.assertRaisesRegex(ValueError, "index is dirty"):
                create_bundle(root, root / "dirty-index.zip")


class ProvenanceTests(unittest.TestCase):
    def test_statement_binds_artifact_and_git_commit(self):
        with tempfile.TemporaryDirectory() as td:
            artifact = Path(td) / "artifact.bin"
            artifact.write_bytes(b"proof")
            sha = "A" * 40
            env = {
                "GITHUB_REPOSITORY": "ejjjkkjlkkj/solution",
                "GITHUB_SHA": sha,
                "GITHUB_REF": "refs/heads/test/provenance-fixture",
                "GITHUB_RUN_ID": "123456",
                "GITHUB_SERVER_URL": "https://github.com",
            }

            statement = build_statement(artifact, env)
            expected = hashlib.sha256(b"proof").hexdigest()
            self.assertEqual(statement["subject"][0]["digest"]["sha256"], expected)

            deps = statement["predicate"]["buildDefinition"]["resolvedDependencies"]
            self.assertEqual(deps[0]["digest"]["gitCommit"], sha.lower())
            self.assertEqual(
                deps[0]["uri"],
                "git+https://github.com/ejjjkkjlkkj/solution.git",
            )
            self.assertEqual(
                statement["predicate"]["runDetails"]["builder"]["id"],
                "https://github.com/ejjjkkjlkkj/solution/actions/runs/123456",
            )

    def test_local_statement_is_explicitly_unresolved(self):
        with tempfile.TemporaryDirectory() as td:
            artifact = Path(td) / "artifact.bin"
            artifact.write_bytes(b"local")
            statement = build_statement(artifact, {})
            deps = statement["predicate"]["buildDefinition"]["resolvedDependencies"]
            self.assertEqual(deps[0]["uri"], "local://unresolved-source")
            self.assertEqual(deps[0]["digest"], {})


if __name__ == "__main__":
    unittest.main()
