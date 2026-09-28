#!/usr/bin/env python3
"""Integrity / confidentiality gate for omni-os.

Checks, against tools/integrity/SOURCES.lock.json:
  1. provenance  - every imported source commit is still in HEAD's history
  2. archives    - every archive branch exists and points at its locked commit
                   (nothing rewritten, nothing deleted)
  3. secrets     - no credential-shaped string in tracked files
  4. size        - no tracked file above the limit (keeps clones available)

Usage: python tools/integrity/verify.py [--archive-prefix refs/remotes/origin/]
Exit code 0 = PASS, 1 = FAIL.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "tools" / "integrity" / "SOURCES.lock.json"
MAX_FILE_BYTES = 50 * 1024 * 1024

SECRET_PATTERNS = [
    re.compile(p)
    for p in (
        r"ghp_[A-Za-z0-9]{36}",
        r"github_pat_[A-Za-z0-9_]{40,}",
        r"gh[osu]_[A-Za-z0-9]{36}",
        r"AKIA[0-9A-Z]{16}",
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        r"xox[baprs]-[A-Za-z0-9-]{10,}",
        r"sk-(?:ant-)?[A-Za-z0-9_-]{32,}",
        r"AIza[0-9A-Za-z_-]{35}",
    )
]


def git(*args):
    return subprocess.run(
        ["git", "-C", str(ROOT), *args], capture_output=True, text=True
    )


def check_provenance(lock, errors):
    for comp in lock["components"]:
        commit = comp["source_commit"]
        if git("merge-base", "--is-ancestor", commit, "HEAD").returncode != 0:
            errors.append(f"provenance: {comp['path']} source {commit[:10]} not in HEAD history")
        if not (ROOT / comp["path"]).is_dir():
            errors.append(f"provenance: component directory {comp['path']}/ missing")


def check_archives(lock, prefix, errors):
    for name, sha in lock["archive_branches"].items():
        ref = prefix + name
        got = git("rev-parse", "-q", "--verify", ref + "^{commit}")
        if got.returncode != 0:
            errors.append(f"archive: {name} missing ({ref})")
        elif got.stdout.strip() != sha:
            errors.append(f"archive: {name} moved {sha[:10]} -> {got.stdout.strip()[:10]}")


def check_files(errors):
    files = git("ls-files", "-z").stdout.split("\0")
    for rel in filter(None, files):
        path = ROOT / rel
        if not path.is_file():
            continue
        size = path.stat().st_size
        if size > MAX_FILE_BYTES:
            errors.append(f"size: {rel} is {size // (1024 * 1024)} MiB (> 50 MiB)")
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        for pat in SECRET_PATTERNS:
            if pat.search(text):
                errors.append(f"secret: {rel} matches {pat.pattern}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--archive-prefix", default="refs/heads/",
                    help="where archive/* refs live (CI: refs/remotes/origin/)")
    args = ap.parse_args()
    lock = json.loads(LOCK.read_text(encoding="utf-8"))
    errors = []
    check_provenance(lock, errors)
    check_archives(lock, args.archive_prefix, errors)
    check_files(errors)
    for e in errors:
        print("FAIL", e)
    print(f"OMNI_OS_INTEGRITY={'FAIL' if errors else 'PASS'} "
          f"components={len(lock['components'])} archives={len(lock['archive_branches'])}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
