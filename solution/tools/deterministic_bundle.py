#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST_NAME = "OMNI-BUNDLE-MANIFEST.json"
SUPPORTED_GIT_MODES = {"100644", "100755"}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def tracked_members(root: pathlib.Path) -> list[tuple[str, bytes, int]]:
    root = root.resolve()
    for diff_args, label in (
        (("diff", "--quiet", "--ignore-submodules=none", "--"), "tracked worktree"),
        (("diff", "--cached", "--quiet", "--ignore-submodules=none", "--"), "index"),
    ):
        clean = subprocess.run(
            ["git", "-C", str(root), *diff_args],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if clean.returncode != 0:
            raise ValueError(f"{label} is dirty")

    proc = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--stage", "-z"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    records = [record for record in proc.stdout.split(b"\0") if record]
    members: list[tuple[str, bytes, int]] = []

    for record in records:
        try:
            meta, path_raw = record.split(b"\t", 1)
            mode_raw, object_raw, stage_raw = meta.split(b" ", 2)
            mode_text = mode_raw.decode("ascii")
            stage = stage_raw.decode("ascii")
            name = path_raw.decode("utf-8")
        except (ValueError, UnicodeDecodeError) as exc:
            raise ValueError("malformed git index entry") from exc

        if stage != "0":
            raise ValueError(f"unmerged git index entry: {name}")
        if mode_text not in SUPPORTED_GIT_MODES:
            raise ValueError(f"unsupported tracked git mode {mode_text}: {name}")

        rel = pathlib.PurePosixPath(name)
        if rel.is_absolute() or ".." in rel.parts:
            raise ValueError(f"unsafe tracked path: {name}")
        if rel.as_posix() == MANIFEST_NAME:
            raise ValueError(f"reserved bundle manifest path is tracked: {MANIFEST_NAME}")

        try:
            object_sha = object_raw.decode("ascii")
        except UnicodeDecodeError as exc:
            raise ValueError(f"invalid Git object id for {name}") from exc
        blob = subprocess.run(
            ["git", "-C", str(root), "cat-file", "blob", object_sha],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ).stdout
        members.append((rel.as_posix(), blob, int(mode_text, 8)))

    members.sort(key=lambda item: item[0])
    if not members:
        raise ValueError("repository has no tracked files")
    return members


def manifest_bytes(members: list[tuple[str, bytes, int]]) -> bytes:
    payload = {
        "schema": "omni.bundle-manifest.v1",
        "files": [
            {
                "path": name,
                "bytes": len(data),
                "sha256": sha256_bytes(data),
            }
            for name, data, _mode in members
        ],
    }
    return (
        json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")


def zip_info(name: str, mode: int = 0o100644) -> zipfile.ZipInfo:
    if mode not in {0o100644, 0o100755}:
        raise ValueError(f"unsupported ZIP member mode for {name}: {mode:o}")
    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_STORED
    info.create_system = 3
    info.external_attr = (mode & 0xFFFF) << 16
    return info


def create_bundle(root: pathlib.Path, output: pathlib.Path) -> str:
    members = tracked_members(root)
    manifest = manifest_bytes(members)

    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(
        output,
        "w",
        compression=zipfile.ZIP_STORED,
        allowZip64=True,
    ) as archive:
        for name, data, mode in members:
            archive.writestr(
                zip_info(name, mode),
                data,
                compress_type=zipfile.ZIP_STORED,
            )
        archive.writestr(
            zip_info(MANIFEST_NAME),
            manifest,
            compress_type=zipfile.ZIP_STORED,
        )

    return hashlib.sha256(output.read_bytes()).hexdigest().upper()


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: deterministic_bundle.py OUTPUT.zip", file=sys.stderr)
        return 2
    try:
        digest = create_bundle(ROOT, pathlib.Path(sys.argv[1]))
    except (OSError, subprocess.CalledProcessError, UnicodeDecodeError, ValueError) as exc:
        print(f"deterministic bundle failed: {exc}", file=sys.stderr)
        return 1
    print(digest)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
