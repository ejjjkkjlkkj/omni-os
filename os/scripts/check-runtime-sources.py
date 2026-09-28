#!/usr/bin/env python3
"""Fail closed on incomplete or falsely release-ready compatibility source inventories."""

from __future__ import annotations

import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "upstreams" / "runtime-sources.toml"

REQUIRED = {
    "common": {
        "ImmutableSourcePins",
        "LicenseInventory",
        "SourceProvenance",
        "ReproducibleBuildRecipe",
    },
    "linux": {
        "LinuxKernel",
        "LinuxLibc",
        "LinuxInitServices",
        "LinuxDbus",
        "LinuxWayland",
        "LinuxXwayland",
        "LinuxMesa",
        "LinuxPipewire",
        "LinuxDesktopPortal",
        "LinuxAccessibilityAtSpi",
    },
    "android": {
        "AndroidBuildSystem",
        "AndroidKernelContract",
        "AndroidBionic",
        "AndroidArt",
        "AndroidBinder",
        "AndroidFrameworkBase",
        "AndroidFrameworkNative",
        "AndroidGraphics",
        "AndroidMediaAudio",
        "AndroidPackageActivityServices",
        "AndroidPermissionSecurity",
        "AndroidAccessibility",
    },
    "darwin": {
        "DarwinMachAbi",
        "DarwinMachOLoader",
        "DarwinDynamicLoader",
        "DarwinLibSystem",
        "DarwinLibc",
        "DarwinObjectiveCRuntime",
        "DarwinDispatch",
        "DarwinCoreFoundationCompat",
        "DarwinFoundationCompat",
        "DarwinAppKitCompat",
        "DarwinGraphicsCompat",
        "DarwinAudioCompat",
        "DarwinSecurityCompat",
        "DarwinAccessibilityCompat",
    },
}

MOVING_PINS = {"", "UNPINNED", "main", "master", "HEAD", "latest"}
ALLOWED_STATUS = {"planned", "ready"}


def fail(message: str) -> None:
    print(f"RUNTIME_SOURCE_MANIFEST = FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    data = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    if data.get("schema") != 1:
        fail("unsupported schema")
    if data.get("host_arch") != "x86_64":
        fail("host architecture must remain x86_64 for this project generation")

    entries = data.get("source")
    if not isinstance(entries, list) or not entries:
        fail("no source entries")

    ids: set[str] = set()
    found = {family: set() for family in REQUIRED}

    for entry in entries:
        source_id = entry.get("id")
        family = entry.get("family")
        component = entry.get("component")
        status = entry.get("status")
        pin = entry.get("pin")
        source_kind = entry.get("source_kind")
        upstream = entry.get("upstream")
        license_text = entry.get("license")

        if not all(isinstance(value, str) and value for value in (
            source_id,
            family,
            component,
            status,
            pin,
            source_kind,
            upstream,
            license_text,
        )):
            fail(f"entry has an empty mandatory field: {entry!r}")
        if source_id in ids:
            fail(f"duplicate source id {source_id}")
        ids.add(source_id)
        if family not in REQUIRED:
            fail(f"unknown family {family} in {source_id}")
        if component not in REQUIRED[family]:
            fail(f"unexpected component {component} for {family}")
        if component in found[family]:
            fail(f"duplicate logical component {family}/{component}")
        found[family].add(component)
        if status not in ALLOWED_STATUS:
            fail(f"invalid status {status} in {source_id}")
        if source_kind == "proprietary":
            fail(f"proprietary source cannot satisfy runtime completeness: {source_id}")
        if family == "darwin" and "proprietary" in source_kind:
            fail(f"Darwin completeness cannot depend on proprietary macOS code: {source_id}")
        if status == "ready" and pin in MOVING_PINS:
            fail(f"ready source {source_id} is not immutably pinned")

    for family, required in REQUIRED.items():
        missing = sorted(required - found[family])
        if missing:
            fail(f"{family} missing components: {', '.join(missing)}")

    if data.get("release_complete"):
        not_ready = sorted(entry["id"] for entry in entries if entry["status"] != "ready")
        if not_ready:
            fail("release_complete=true while sources are not ready: " + ", ".join(not_ready))
        moving = sorted(entry["id"] for entry in entries if entry["pin"] in MOVING_PINS)
        if moving:
            fail("release_complete=true with moving/unset pins: " + ", ".join(moving))

    print("RUNTIME_SOURCE_MANIFEST = PASS")
    print(f"RUNTIME_SOURCE_ENTRIES = {len(entries)}")
    for family in ("linux", "android", "darwin", "common"):
        print(f"{family.upper()}_SOURCE_COMPONENTS = {len(found[family])}")
    print(f"RELEASE_COMPLETE = {str(bool(data.get('release_complete'))).upper()}")


if __name__ == "__main__":
    main()
