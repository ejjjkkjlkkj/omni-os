#!/usr/bin/env python3
"""Resolve and download the complete source closure of the shipped Linux guest runtime.

The resolver uses an isolated APT state rooted under --dest. It does not install packages on the
host. Runtime binary dependencies are downloaded first. Every binary is mapped back to its Debian
source package. Build dependencies for every discovered source package are then downloaded, mapped
back to source packages, and processed recursively until a fixed point is reached.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK_PATH = ROOT / "upstreams" / "linux-runtime.lock.toml"
SEED_PATH = ROOT / "upstreams" / "linux-runtime-packages.txt"
SOURCE_RE = re.compile(r"^([^ ]+)(?: \(([^)]+)\))?$")


class ClosureError(RuntimeError):
    pass


def run(command: list[str], *, cwd: pathlib.Path | None = None, capture: bool = False) -> str:
    print("+", " ".join(command))
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    return completed.stdout if capture else ""


def require_tool(name: str) -> None:
    if shutil.which(name) is None:
        raise ClosureError(f"required tool not found: {name}")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_lock() -> dict:
    return tomllib.loads(LOCK_PATH.read_text(encoding="utf-8"))


def load_seeds() -> list[str]:
    seeds = []
    for line in SEED_PATH.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            seeds.append(line)
    if not seeds:
        raise ClosureError("Linux runtime seed package list is empty")
    return seeds


def apt_options(apt_root: pathlib.Path, sources_list: pathlib.Path, architecture: str) -> list[str]:
    return [
        "-o",
        f"Dir::Etc::sourcelist={sources_list}",
        "-o",
        "Dir::Etc::sourceparts=-",
        "-o",
        f"Dir::State={apt_root / 'var/lib/apt'}",
        "-o",
        f"Dir::State::status={apt_root / 'var/lib/dpkg/status'}",
        "-o",
        f"Dir::Cache={apt_root / 'var/cache/apt'}",
        "-o",
        f"APT::Architecture={architecture}",
        "-o",
        "Acquire::Check-Valid-Until=false",
    ]


def prepare_apt_root(dest: pathlib.Path, lock: dict) -> tuple[pathlib.Path, pathlib.Path, list[str]]:
    userspace = lock["userspace"]
    snapshot = userspace["snapshot"]
    if snapshot in {"", "UNPINNED"}:
        raise ClosureError("Debian snapshot is not pinned")

    apt_root = dest / "apt-root"
    for directory in (
        apt_root / "etc/apt",
        apt_root / "var/lib/apt/lists/partial",
        apt_root / "var/lib/dpkg",
        apt_root / "var/cache/apt/archives/partial",
    ):
        directory.mkdir(parents=True, exist_ok=True)
    (apt_root / "var/lib/dpkg/status").touch()

    source_list = apt_root / "etc/apt/sources.list"
    main = userspace["main_snapshot"]
    security = userspace["security_snapshot"]
    components = " ".join(userspace["components"])
    lines = [
        f"deb [check-valid-until=no] {main} trixie {components}",
        f"deb-src [check-valid-until=no] {main} trixie {components}",
        f"deb [check-valid-until=no] {main} trixie-updates {components}",
        f"deb-src [check-valid-until=no] {main} trixie-updates {components}",
        f"deb [check-valid-until=no] {security} trixie-security {components}",
        f"deb-src [check-valid-until=no] {security} trixie-security {components}",
    ]
    source_list.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return apt_root, source_list, apt_options(apt_root, source_list, userspace["architecture"])


def deb_field(deb: pathlib.Path, field: str) -> str:
    return run(["dpkg-deb", "-f", str(deb), field], capture=True).strip()


def source_identity(deb: pathlib.Path) -> tuple[str, str]:
    binary_name = deb_field(deb, "Package")
    binary_version = deb_field(deb, "Version")
    source = deb_field(deb, "Source")
    if not source:
        return binary_name, binary_version
    match = SOURCE_RE.fullmatch(source)
    if match is None:
        raise ClosureError(f"cannot parse Source field {source!r} from {deb.name}")
    return match.group(1), match.group(2) or binary_version


def inspect_debs(archives: pathlib.Path) -> tuple[list[dict[str, str]], set[tuple[str, str]]]:
    binaries: list[dict[str, str]] = []
    sources: set[tuple[str, str]] = set()
    for deb in sorted(archives.glob("*.deb")):
        package = deb_field(deb, "Package")
        version = deb_field(deb, "Version")
        architecture = deb_field(deb, "Architecture")
        source_name, source_version = source_identity(deb)
        binaries.append(
            {
                "package": package,
                "version": version,
                "architecture": architecture,
                "source": source_name,
                "source_version": source_version,
                "file": deb.name,
                "sha256": sha256(deb),
            }
        )
        sources.add((source_name, source_version))
    return binaries, sources


def download_source(
    apt_opts: list[str], source_dir: pathlib.Path, source: str, version: str
) -> None:
    run(
        ["apt-get", *apt_opts, "source", "--download-only", f"{source}={version}"],
        cwd=source_dir,
    )


def download_build_dependencies(
    apt_opts: list[str], source: str, version: str
) -> None:
    run(
        [
            "apt-get",
            *apt_opts,
            "--download-only",
            "--no-install-recommends",
            "-y",
            "build-dep",
            f"{source}={version}",
        ]
    )


def save_source_metadata(
    apt_opts: list[str], metadata_dir: pathlib.Path, source: str, version: str
) -> None:
    text = run(["apt-cache", *apt_opts, "showsrc", f"{source}={version}"], capture=True)
    safe_version = version.replace(":", "_")
    (metadata_dir / f"{source}_{safe_version}.sources.txt").write_text(text, encoding="utf-8")


def build_manifest(dest: pathlib.Path, binaries: list[dict[str, str]]) -> None:
    source_dir = dest / "sources"
    source_files = []
    for path in sorted(source_dir.iterdir()):
        if path.is_file():
            source_files.append(
                {
                    "file": path.name,
                    "size": path.stat().st_size,
                    "sha256": sha256(path),
                }
            )
    manifest = {
        "schema": 1,
        "binary_packages": sorted(binaries, key=lambda item: (item["package"], item["version"])),
        "source_files": source_files,
    }
    (dest / "linux-runtime-source-closure.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dest", required=True, type=pathlib.Path)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    lock = load_lock()
    seeds = load_seeds()
    if args.dry_run:
        userspace = lock["userspace"]
        if userspace["snapshot"] in {"", "UNPINNED"}:
            raise SystemExit("LINUX_SOURCE_CLOSURE = FAIL: snapshot is not pinned")
        print("LINUX_SOURCE_CLOSURE_DRY_RUN = PASS")
        print(f"SNAPSHOT = {userspace['snapshot']}")
        print(f"SEED_PACKAGES = {len(seeds)}")
        print("RUNTIME_DEPENDENCIES = TRANSITIVE")
        print("BUILD_DEPENDENCIES = TRANSITIVE_FIXED_POINT")
        return

    try:
        for tool in ("apt-get", "apt-cache", "dpkg-deb"):
            require_tool(tool)
        dest = args.dest.expanduser().resolve()
        dest.mkdir(parents=True, exist_ok=True)
        apt_root, sources_list, apt_opts = prepare_apt_root(dest, lock)
        source_dir = dest / "sources"
        metadata_dir = dest / "source-metadata"
        source_dir.mkdir(exist_ok=True)
        metadata_dir.mkdir(exist_ok=True)

        run(["apt-get", *apt_opts, "update"])
        run(
            [
                "apt-get",
                *apt_opts,
                "--download-only",
                "--no-install-recommends",
                "-y",
                "install",
                *seeds,
            ]
        )

        archives = apt_root / "var/cache/apt/archives"
        processed_sources: set[tuple[str, str]] = set()
        all_binaries: list[dict[str, str]] = []

        while True:
            binaries, discovered_sources = inspect_debs(archives)
            all_binaries = binaries
            pending = sorted(discovered_sources - processed_sources)
            if not pending:
                break
            for source, version in pending:
                print(f"SOURCE_CLOSURE_PROCESS = {source} {version}")
                download_source(apt_opts, source_dir, source, version)
                save_source_metadata(apt_opts, metadata_dir, source, version)
                download_build_dependencies(apt_opts, source, version)
                processed_sources.add((source, version))

        build_manifest(dest, all_binaries)
        shutil.copy2(LOCK_PATH, dest / LOCK_PATH.name)
        shutil.copy2(SEED_PATH, dest / SEED_PATH.name)
        shutil.copy2(sources_list, dest / "sources.list")
        print("LINUX_SOURCE_CLOSURE = PASS")
        print(f"BINARY_PACKAGES = {len(all_binaries)}")
        print(f"SOURCE_PACKAGES = {len(processed_sources)}")
        print(f"OUTPUT = {dest / 'linux-runtime-source-closure.json'}")
    except (ClosureError, OSError, subprocess.CalledProcessError) as exc:
        print(f"LINUX_SOURCE_CLOSURE = FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc


if __name__ == "__main__":
    main()
