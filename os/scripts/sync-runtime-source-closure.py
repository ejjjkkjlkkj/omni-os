#!/usr/bin/env python3
"""Synchronize pinned compatibility-runtime source closures.

This script intentionally fails closed. It never converts a moving branch or an UNPINNED
placeholder into release provenance. Large source trees are stored outside the Git repository in a
caller-selected destination.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
UPSTREAMS = ROOT / "upstreams"


class SyncError(RuntimeError):
    pass


def load_toml(name: str) -> dict:
    return tomllib.loads((UPSTREAMS / name).read_text(encoding="utf-8"))


def run(command: list[str], *, cwd: pathlib.Path | None, dry_run: bool) -> None:
    print("+", " ".join(command))
    if not dry_run:
        subprocess.run(command, cwd=cwd, check=True)


def require_tool(name: str, *, dry_run: bool) -> None:
    if not dry_run and shutil.which(name) is None:
        raise SyncError(f"required tool not found: {name}")


def ensure_empty_or_existing_repo(path: pathlib.Path) -> None:
    if path.exists() and any(path.iterdir()) and not (path / ".git").exists() and not (path / ".repo").exists():
        raise SyncError(f"refusing to reuse non-empty non-repository directory: {path}")
    path.mkdir(parents=True, exist_ok=True)


def sync_android(dest: pathlib.Path, *, dry_run: bool, jobs: int) -> None:
    lock = load_toml("android-aosp.lock.toml")
    manifest = lock["platform_manifest"]
    tag = manifest["tag"]
    expected_manifest_commit = manifest["commit"]
    android = dest / "android-aosp"
    ensure_empty_or_existing_repo(android)
    require_tool("repo", dry_run=dry_run)
    require_tool("git", dry_run=dry_run)

    if not (android / ".repo").exists():
        run(
            [
                "repo",
                "init",
                "-u",
                manifest["url"],
                "-b",
                tag,
                "--no-clone-bundle",
            ],
            cwd=android,
            dry_run=dry_run,
        )

    if not dry_run:
        actual_manifest_commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=android / ".repo" / "manifests",
            text=True,
        ).strip()
        if actual_manifest_commit != expected_manifest_commit:
            raise SyncError(
                "AOSP manifest commit mismatch: "
                f"expected {expected_manifest_commit}, got {actual_manifest_commit}"
            )

    run(
        ["repo", "sync", "-c", "--no-clone-bundle", "--force-sync", "-j", str(jobs)],
        cwd=android,
        dry_run=dry_run,
    )
    resolved = android / "resolved-manifest.xml"
    run(
        ["repo", "manifest", "-r", "-o", str(resolved)],
        cwd=android,
        dry_run=dry_run,
    )
    print(f"ANDROID_RESOLVED_MANIFEST = {resolved}")


def sync_linux(dest: pathlib.Path, *, dry_run: bool) -> None:
    lock = load_toml("linux-runtime.lock.toml")
    kernel = lock["kernel"]
    userspace = lock["userspace"]
    if userspace["snapshot"] == "UNPINNED":
        raise SyncError(
            "Linux userspace snapshot is still UNPINNED; refusing to create an incomplete "
            "release source closure"
        )

    require_tool("git", dry_run=dry_run)
    linux = dest / "linux-runtime"
    linux.mkdir(parents=True, exist_ok=True)
    kernel_dir = linux / "kernel"
    if not kernel_dir.exists():
        run(
            [
                "git",
                "clone",
                "--filter=blob:none",
                "--branch",
                kernel["tag"],
                "--single-branch",
                kernel["source"],
                str(kernel_dir),
            ],
            cwd=None,
            dry_run=dry_run,
        )
    print(f"LINUX_KERNEL_SOURCE = {kernel_dir}")
    print("LINUX_USERSPACE_SOURCE_CLOSURE = PINNED_SNAPSHOT_REQUIRED")


def clone_at_tag(repo: str, tag: str, path: pathlib.Path, *, dry_run: bool) -> None:
    if path.exists():
        return
    run(
        ["git", "clone", "--filter=blob:none", "--branch", tag, "--single-branch", repo, str(path)],
        cwd=None,
        dry_run=dry_run,
    )


def clone_at_commit(
    repo: str,
    commit: str,
    path: pathlib.Path,
    *,
    dry_run: bool,
    recursive_submodules: bool,
) -> list[str]:
    if path.exists() and not (path / ".git").exists():
        raise SyncError(f"refusing to reuse non-Git compatibility source directory: {path}")

    fresh_clone = not path.exists()
    if fresh_clone:
        run(
            ["git", "clone", "--filter=blob:none", "--no-checkout", repo, str(path)],
            cwd=None,
            dry_run=dry_run,
        )
    elif not dry_run:
        actual_remote = subprocess.check_output(
            ["git", "remote", "get-url", "origin"], cwd=path, text=True
        ).strip()
        if actual_remote != repo:
            raise SyncError(
                f"compatibility source origin mismatch for {path}: expected {repo}, got {actual_remote}"
            )
        run(["git", "fetch", "--tags", "--prune", "origin"], cwd=path, dry_run=False)

    run(["git", "checkout", "--detach", commit], cwd=path, dry_run=dry_run)
    if recursive_submodules:
        run(
            ["git", "submodule", "update", "--init", "--recursive"],
            cwd=path,
            dry_run=dry_run,
        )

    if dry_run:
        return []

    actual_commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=path, text=True
    ).strip()
    if actual_commit != commit:
        raise SyncError(
            f"compatibility source commit mismatch for {path}: expected {commit}, got {actual_commit}"
        )

    if not recursive_submodules:
        return []
    status = subprocess.check_output(
        ["git", "submodule", "status", "--recursive"], cwd=path, text=True
    )
    return [line.strip() for line in status.splitlines() if line.strip()]


def sync_darwin_apple_oss(dest: pathlib.Path, *, dry_run: bool) -> None:
    lock = load_toml("darwin-open-source.lock.toml")
    inventory = lock["apple_open_source_inventory"]
    require_tool("git", dry_run=dry_run)

    darwin = dest / "darwin-open-source"
    darwin.mkdir(parents=True, exist_ok=True)
    inventory_dir = darwin / "distribution-macOS"
    if not inventory_dir.exists():
        run(
            ["git", "clone", inventory["repository"], str(inventory_dir)],
            cwd=None,
            dry_run=dry_run,
        )
    run(["git", "checkout", "--detach", inventory["commit"]], cwd=inventory_dir, dry_run=dry_run)

    if dry_run:
        print("DARWIN_APPLE_OSS_COMPONENT_SYNC = requires release.json after inventory checkout")
        return

    actual_inventory_commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=inventory_dir, text=True
    ).strip()
    if actual_inventory_commit != inventory["commit"]:
        raise SyncError(
            "Apple OSS inventory commit mismatch: "
            f"expected {inventory['commit']}, got {actual_inventory_commit}"
        )

    release_path = inventory_dir / "release.json"
    release = json.loads(release_path.read_text(encoding="utf-8"))
    projects = release.get("projects")
    if not isinstance(projects, list):
        raise SyncError("Apple OSS release.json does not contain a projects list")

    components_dir = darwin / "apple-components"
    components_dir.mkdir(exist_ok=True)
    resolved: list[dict[str, str]] = []
    for project in projects:
        name = project.get("project")
        tag = project.get("tag")
        if not isinstance(name, str) or not isinstance(tag, str):
            raise SyncError(f"invalid Apple OSS project entry: {project!r}")
        repo = f"https://github.com/apple-oss-distributions/{name}.git"
        target = components_dir / name
        clone_at_tag(repo, tag, target, dry_run=False)
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=target, text=True).strip()
        resolved.append({"project": name, "tag": tag, "commit": commit, "repository": repo})

    resolved_path = darwin / "resolved-apple-oss.json"
    resolved_path.write_text(json.dumps(resolved, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"DARWIN_APPLE_OSS_RESOLVED = {resolved_path}")
    print(f"DARWIN_APPLE_OSS_PROJECTS = {len(resolved)}")


def sync_darwin_compatibility(dest: pathlib.Path, *, dry_run: bool) -> None:
    lock = load_toml("darwin-open-source.lock.toml")
    compatibility = lock["compatibility_projects"]
    require_tool("git", dry_run=dry_run)

    compatibility_dir = dest / "darwin-open-source" / "compatibility"
    compatibility_dir.mkdir(parents=True, exist_ok=True)
    projects = (
        (
            "darling",
            compatibility["darling"],
            compatibility["darling_commit"],
            True,
        ),
        (
            "libobjc2",
            compatibility["objc_runtime"],
            compatibility["objc_runtime_commit"],
            True,
        ),
        (
            "gnustep-base",
            compatibility["foundation"],
            compatibility["foundation_commit"],
            True,
        ),
        (
            "gnustep-gui",
            compatibility["appkit"],
            compatibility["appkit_commit"],
            True,
        ),
    )

    resolved: list[dict[str, object]] = []
    for name, repo, commit, recursive_submodules in projects:
        target = compatibility_dir / name
        submodules = clone_at_commit(
            repo,
            commit,
            target,
            dry_run=dry_run,
            recursive_submodules=recursive_submodules,
        )
        resolved.append(
            {
                "project": name,
                "repository": repo,
                "commit": commit,
                "submodules": submodules,
            }
        )

    resolved_path = compatibility_dir / "resolved-compatibility-projects.json"
    if not dry_run:
        resolved_path.write_text(
            json.dumps(resolved, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    print(f"DARWIN_COMPATIBILITY_RESOLVED = {resolved_path}")
    print(f"DARWIN_COMPATIBILITY_PROJECTS = {len(projects)}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "family",
        choices=(
            "android",
            "linux",
            "darwin-apple-oss",
            "darwin-compat",
            "darwin",
            "all",
        ),
    )
    parser.add_argument("--dest", type=pathlib.Path, required=True)
    parser.add_argument("--jobs", type=int, default=max(1, min(16, os.cpu_count() or 1)))
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    dest = args.dest.expanduser().resolve()
    dest.mkdir(parents=True, exist_ok=True)
    try:
        if args.family in ("android", "all"):
            sync_android(dest, dry_run=args.dry_run, jobs=args.jobs)
        if args.family in ("linux", "all"):
            sync_linux(dest, dry_run=args.dry_run)
        if args.family in ("darwin-apple-oss", "darwin", "all"):
            sync_darwin_apple_oss(dest, dry_run=args.dry_run)
        if args.family in ("darwin-compat", "darwin", "all"):
            sync_darwin_compatibility(dest, dry_run=args.dry_run)
    except (OSError, subprocess.CalledProcessError, SyncError) as exc:
        print(f"RUNTIME_SOURCE_SYNC = FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
    print("RUNTIME_SOURCE_SYNC = PASS")


if __name__ == "__main__":
    main()
