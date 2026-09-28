#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import pathlib
import secrets

from tools.verify_uefi_evidence import EvidenceError, SHA256_RE, sha256_file


def _inside(path: pathlib.Path, root: pathlib.Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def _write_sync(path: pathlib.Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="ascii", newline="\n") as stream:
        stream.write(text)
        stream.flush()
        os.fsync(stream.fileno())


def prepare(
    mount: pathlib.Path,
    expected_sha256: str,
    challenge_out: pathlib.Path,
    challenge: str | None = None,
) -> dict[str, str]:
    mount = mount.resolve()
    if not mount.is_dir():
        raise EvidenceError(f"media mount is not a directory: {mount}")
    if not SHA256_RE.fullmatch(expected_sha256):
        raise EvidenceError("expected SHA-256 must be 64 hexadecimal characters")

    efi = mount / "EFI" / "BOOT" / "BOOTX64.EFI"
    if not efi.is_file():
        raise EvidenceError(f"BOOTX64.EFI not found: {efi}")
    actual_sha = sha256_file(efi)
    expected_sha256 = expected_sha256.lower()
    if actual_sha != expected_sha256:
        raise EvidenceError(
            f"EFI SHA-256 mismatch before boot: actual={actual_sha} expected={expected_sha256}"
        )

    challenge_out = challenge_out.resolve()
    if _inside(challenge_out, mount):
        raise EvidenceError("challenge-out must be stored off the boot media")

    challenge = challenge or secrets.token_hex(32)
    if not SHA256_RE.fullmatch(challenge):
        raise EvidenceError("challenge must be 64 hexadecimal characters")
    challenge = challenge.lower()

    evidence = mount / "OMNI-EVIDENCE.TXT"
    evidence.unlink(missing_ok=True)

    challenge_path = mount / "OMNI-CHALLENGE.TXT"
    _write_sync(challenge_path, challenge + "\n")
    _write_sync(challenge_out, challenge + "\n")

    return {
        "status": "PHYSICAL_MEDIA_PREPARED",
        "efi_sha256": actual_sha,
        "challenge": challenge,
        "challenge_file": str(challenge_path),
        "expected_challenge_file": str(challenge_out),
    }


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Prepare OmniProbe removable media with a fresh challenge."
    )
    ap.add_argument("--mount", required=True, type=pathlib.Path)
    ap.add_argument("--expected-sha256", required=True)
    ap.add_argument("--challenge-out", required=True, type=pathlib.Path)
    ns = ap.parse_args()
    try:
        result = prepare(ns.mount, ns.expected_sha256, ns.challenge_out)
    except (OSError, EvidenceError) as exc:
        print(json.dumps({"status": "FAIL", "error": str(exc)}, sort_keys=True))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
