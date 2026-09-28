#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import zipfile

MANIFEST_NAME = "OMNI-BUNDLE-MANIFEST.json"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
DIGEST_RE = re.compile(r"^[0-9a-f]{64}$")


def _git(root: pathlib.Path, *args: str) -> bytes:
    return subprocess.check_output(
        ["git", "-C", str(root), *args],
        stderr=subprocess.PIPE,
    )


def verify_bundle_commit(
    root: pathlib.Path,
    bundle: pathlib.Path,
    expected_commit: str,
) -> dict[str, object]:
    failures: list[str] = []
    root = root.resolve()
    expected = expected_commit.strip().lower()

    try:
        commit = _git(root, "rev-parse", "--verify", f"{expected}^{{commit}}").decode(
            "ascii"
        ).strip().lower()
    except (subprocess.CalledProcessError, UnicodeDecodeError):
        commit = ""
        failures.append("COMMIT_UNRESOLVED")

    if not SHA_RE.fullmatch(expected):
        failures.append("EXPECTED_COMMIT_INVALID")
    if commit != expected:
        failures.append("COMMIT_MISMATCH")

    try:
        tree_raw = _git(root, "ls-tree", "-r", "-z", "--full-tree", commit)
        tracked: dict[str, tuple[str, int]] = {}
        for record in (item for item in tree_raw.split(b"\0") if item):
            meta, path_raw = record.split(b"\t", 1)
            mode_raw, kind_raw, object_raw = meta.split(b" ", 2)
            name = path_raw.decode("utf-8")
            mode = mode_raw.decode("ascii")
            kind = kind_raw.decode("ascii")
            object_sha = object_raw.decode("ascii")
            if kind != "blob" or mode not in {"100644", "100755"}:
                failures.append(f"UNSUPPORTED_TREE_ENTRY:{name}")
                continue
            tracked[name] = (object_sha, int(mode, 8))

        with zipfile.ZipFile(bundle, "r") as archive:
            names = archive.namelist()
            if len(names) != len(set(names)):
                failures.append("DUPLICATE_ZIP_MEMBER")
            if MANIFEST_NAME not in names:
                failures.append("MANIFEST_MISSING")
                manifest = {}
            else:
                manifest = json.loads(archive.read(MANIFEST_NAME).decode("utf-8"))

            entries = manifest.get("files") if isinstance(manifest, dict) else None
            if manifest.get("schema") != "omni.bundle-manifest.v1":
                failures.append("MANIFEST_SCHEMA_INVALID")
            if not isinstance(entries, list):
                failures.append("MANIFEST_FILES_INVALID")
                entries = []

            manifest_paths: set[str] = set()
            for index, entry in enumerate(entries):
                if not isinstance(entry, dict):
                    failures.append(f"MANIFEST_ENTRY_{index}_INVALID")
                    continue
                name = entry.get("path")
                digest = entry.get("sha256")
                size = entry.get("bytes")
                if not isinstance(name, str) or name in manifest_paths:
                    failures.append(f"MANIFEST_ENTRY_{index}_PATH_INVALID")
                    continue
                manifest_paths.add(name)
                if name not in tracked:
                    failures.append(f"UNTRACKED_ARCHIVE_MEMBER:{name}")
                    continue
                if not isinstance(digest, str) or not DIGEST_RE.fullmatch(digest):
                    failures.append(f"MANIFEST_ENTRY_{index}_DIGEST_INVALID")
                    continue
                if not isinstance(size, int) or isinstance(size, bool) or size < 0:
                    failures.append(f"MANIFEST_ENTRY_{index}_SIZE_INVALID")
                    continue
                try:
                    archived = archive.read(name)
                except KeyError:
                    failures.append(f"ARCHIVE_MEMBER_MISSING:{name}")
                    continue
                object_sha, expected_mode = tracked[name]
                committed = _git(root, "cat-file", "blob", object_sha)
                actual_mode = (archive.getinfo(name).external_attr >> 16) & 0xFFFF
                if actual_mode != expected_mode:
                    failures.append(f"COMMIT_MODE_MISMATCH:{name}")
                actual_digest = hashlib.sha256(archived).hexdigest()
                committed_digest = hashlib.sha256(committed).hexdigest()
                if archived != committed:
                    failures.append(f"COMMIT_CONTENT_MISMATCH:{name}")
                if len(archived) != size:
                    failures.append(f"ARCHIVE_SIZE_MISMATCH:{name}")
                if actual_digest != digest:
                    failures.append(f"MANIFEST_DIGEST_MISMATCH:{name}")
                if committed_digest != digest:
                    failures.append(f"COMMIT_DIGEST_MISMATCH:{name}")

            if manifest_paths != set(tracked):
                failures.append("TRACKED_FILE_SET_MISMATCH")
            expected_zip_names = set(tracked) | {MANIFEST_NAME}
            if set(names) != expected_zip_names:
                failures.append("ZIP_MEMBER_SET_MISMATCH")

    except (
        OSError,
        ValueError,
        UnicodeDecodeError,
        json.JSONDecodeError,
        zipfile.BadZipFile,
        subprocess.CalledProcessError,
    ) as exc:
        failures.append(f"VERIFY_EXCEPTION:{type(exc).__name__}")

    return {
        "status": "PASS" if not failures else "FAIL",
        "commit": commit or None,
        "bundle_sha256": (
            hashlib.sha256(bundle.read_bytes()).hexdigest().upper()
            if bundle.is_file()
            else None
        ),
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle", type=pathlib.Path)
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."))
    parser.add_argument("--expect-commit", required=True)
    args = parser.parse_args()
    result = verify_bundle_commit(args.root, args.bundle, args.expect_commit)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
