#!/usr/bin/env python3
"""Validate family lockfiles without pretending unfinished source closures are release ready."""

from __future__ import annotations

import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
UPSTREAMS = ROOT / "upstreams"
SHA40 = re.compile(r"^[0-9a-f]{40}$")


def fail(message: str) -> None:
    print(f"RUNTIME_SOURCE_LOCKS = FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def load(name: str) -> dict:
    path = UPSTREAMS / name
    if not path.is_file():
        fail(f"missing {name}")
    return tomllib.loads(path.read_text(encoding="utf-8"))


def ensure_false(gate: dict, key: str, context: str) -> None:
    if gate.get(key) is not False:
        fail(f"{context}: {key} must remain false until real validation passes")


def check_android() -> None:
    data = load("android-aosp.lock.toml")
    if data.get("family") != "android" or data.get("source_policy") != "full-manifest-closure":
        fail("Android lock policy mismatch")
    manifest = data.get("platform_manifest", {})
    if not SHA40.fullmatch(str(manifest.get("tag_object", ""))):
        fail("Android manifest tag object is not a 40-hex immutable id")
    if not SHA40.fullmatch(str(manifest.get("commit", ""))):
        fail("Android manifest commit is not a 40-hex immutable id")
    requirements = data.get("requirements", {})
    for key in (
        "repo_manifest_is_source_of_truth",
        "fetch_every_manifest_project",
        "record_every_project_commit",
        "retain_project_license_metadata",
        "generate_sbom",
        "generate_source_provenance",
        "accessibility_framework_required",
    ):
        if requirements.get(key) is not True:
            fail(f"Android requirement {key} must be true")
    if requirements.get("allow_proprietary_google_packages") is not False:
        fail("Android baseline must not silently include proprietary Google packages")
    if requirements.get("allow_untracked_prebuilt_runtime_dependency") is not False:
        fail("Android baseline must reject untracked runtime prebuilts")
    gate = data.get("release_gate", {})
    if gate.get("status") not in {"inventory-pinned", "ready"}:
        fail("Android release gate has invalid status")
    if gate.get("status") != "ready":
        for key in ("build_verified", "runtime_boot_verified", "accessibility_verified"):
            ensure_false(gate, key, "Android")


def check_linux() -> None:
    data = load("linux-runtime.lock.toml")
    if data.get("family") != "linux":
        fail("Linux lock family mismatch")
    kernel = data.get("kernel", {})
    if not str(kernel.get("tag", "")).startswith("v"):
        fail("Linux kernel must use a release tag")
    if kernel.get("verify_release_signature") is not True:
        fail("Linux kernel release signature verification is mandatory")
    userspace = data.get("userspace", {})
    gate = data.get("release_gate", {})
    if userspace.get("snapshot") == "UNPINNED" and gate.get("status") == "ready":
        fail("Linux cannot be ready with an unpinned distribution snapshot")
    closure = data.get("closure", {})
    for key in (
        "record_every_installed_binary_package",
        "map_every_binary_to_source_package",
        "fetch_corresponding_source_packages",
        "record_source_checksums",
        "record_build_dependencies",
        "record_runtime_dependencies",
        "retain_copyright_and_license_files",
        "generate_sbom",
        "generate_source_provenance",
    ):
        if closure.get(key) is not True:
            fail(f"Linux closure requirement {key} must be true")
    if closure.get("allow_binary_without_source_provenance") is not False:
        fail("Linux must reject binaries without source provenance")
    if gate.get("status") != "ready":
        for key in ("build_verified", "runtime_boot_verified", "accessibility_verified"):
            ensure_false(gate, key, "Linux")


def check_darwin() -> None:
    data = load("darwin-open-source.lock.toml")
    if data.get("family") != "darwin":
        fail("Darwin lock family mismatch")
    inventory = data.get("apple_open_source_inventory", {})
    if not SHA40.fullmatch(str(inventory.get("commit", ""))):
        fail("Darwin Apple OSS inventory must be immutably pinned")

    compatibility = data.get("compatibility_projects", {})
    for repository_key, commit_key in (
        ("darling", "darling_commit"),
        ("objc_runtime", "objc_runtime_commit"),
        ("foundation", "foundation_commit"),
        ("appkit", "appkit_commit"),
    ):
        repository = str(compatibility.get(repository_key, ""))
        commit = str(compatibility.get(commit_key, ""))
        if not repository.startswith("https://github.com/") or not repository.endswith(".git"):
            fail(f"Darwin compatibility repository {repository_key} must be an explicit GitHub clone URL")
        if not SHA40.fullmatch(commit):
            fail(f"Darwin compatibility source {commit_key} must be a 40-hex immutable id")

    closure = data.get("closure", {})
    for key in (
        "walk_all_projects_in_apple_distribution_inventory",
        "record_component_tag_and_commit",
        "record_component_license",
        "record_compatibility_project_commit",
        "record_clean_room_implementation_commit",
        "generate_sbom",
        "generate_source_provenance",
    ):
        if closure.get(key) is not True:
            fail(f"Darwin closure requirement {key} must be true")
    for key in (
        "allow_proprietary_macos_frameworks",
        "allow_macos_system_image",
        "allow_unlicensed_apple_binary",
    ):
        if closure.get(key) is not False:
            fail(f"Darwin safety rule {key} must be false")
    gate = data.get("release_gate", {})
    if gate.get("status") != "ready":
        for key in (
            "source_inventory_verified",
            "mach_o_loader_verified",
            "cli_compatibility_verified",
            "gui_compatibility_verified",
            "accessibility_verified",
        ):
            ensure_false(gate, key, "Darwin")


def main() -> None:
    check_android()
    check_linux()
    check_darwin()
    print("RUNTIME_SOURCE_LOCKS = PASS")
    print("ANDROID_FULL_MANIFEST_BASELINE = PINNED")
    print("LINUX_FULL_SOURCE_CLOSURE = NOT_READY")
    print("DARWIN_OPEN_SOURCE_INVENTORY = PINNED")
    print("DARWIN_COMPATIBILITY_PROJECTS = PINNED")
    print("DARWIN_FULL_COMPATIBILITY_CLOSURE = NOT_READY")


if __name__ == "__main__":
    main()
