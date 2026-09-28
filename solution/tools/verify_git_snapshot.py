#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess

SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def _run(root: pathlib.Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(root), *args],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def verify_snapshot(root: pathlib.Path, expected_commit: str) -> dict[str, object]:
    root = root.resolve()
    expected = expected_commit.strip().lower()
    failures: list[str] = []

    if not SHA_RE.fullmatch(expected):
        failures.append("EXPECTED_COMMIT_INVALID")

    head_run = _run(root, "rev-parse", "--verify", "HEAD")
    head = head_run.stdout.strip().lower() if head_run.returncode == 0 else ""
    if not SHA_RE.fullmatch(head):
        failures.append("HEAD_UNRESOLVED")
    elif SHA_RE.fullmatch(expected) and head != expected:
        failures.append("HEAD_COMMIT_MISMATCH")

    worktree = _run(root, "diff", "--quiet", "--ignore-submodules=none", "--")
    if worktree.returncode != 0:
        failures.append("TRACKED_WORKTREE_DIRTY")

    index = _run(root, "diff", "--cached", "--quiet", "--ignore-submodules=none", "--")
    if index.returncode != 0:
        failures.append("INDEX_DIRTY")

    status = _run(root, "status", "--porcelain=v1", "--untracked-files=no")
    if status.returncode != 0:
        failures.append("GIT_STATUS_FAILED")
    elif status.stdout:
        failures.append("TRACKED_STATUS_DIRTY")

    return {
        "status": "PASS" if not failures else "FAIL",
        "expected_commit": expected,
        "head_commit": head or None,
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."))
    parser.add_argument("--expect-commit", required=True)
    args = parser.parse_args()
    result = verify_snapshot(args.root, args.expect_commit)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
