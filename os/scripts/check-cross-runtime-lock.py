#!/usr/bin/env python3
"""Validate VMM, ISA translation and host-broker source baselines."""

from __future__ import annotations

import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK = ROOT / "upstreams" / "cross-runtime.lock.toml"
SHA40 = re.compile(r"^[0-9a-f]{40}$")


def fail(message: str) -> None:
    print(f"CROSS_RUNTIME_LOCK = FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    data = tomllib.loads(LOCK.read_text(encoding="utf-8"))
    if data.get("schema") != 1 or data.get("scope") != "cross-runtime":
        fail("schema/scope mismatch")

    vmm = data.get("vmm", {})
    if vmm.get("implementation") != "crosvm":
        fail("crosvm is the selected initial VMM baseline")
    if not SHA40.fullmatch(str(vmm.get("commit", ""))):
        fail("VMM must be pinned by exact commit")
    if vmm.get("license_review_required") is not True:
        fail("VMM license review cannot be disabled")

    translator = data.get("foreign_isa_translation", {})
    if translator.get("host_isa") != "x86_64" or translator.get("target_guest_isa") != "aarch64":
        fail("translation baseline must cover AArch64 guests on the x86_64 host")
    if translator.get("release_signature_required") is not True:
        fail("QEMU release signature verification is mandatory")
    for key in (
        "android_native_bridge_adapter_required",
        "linux_user_mode_adapter_required",
        "darwin_mach_o_adapter_required",
    ):
        if translator.get(key) is not True:
            fail(f"translation adapter requirement disabled: {key}")

    bridges = data.get("host_bridges", {})
    required_bridges = {
        "windowing",
        "filesystem",
        "clipboard",
        "notifications",
        "audio",
        "network",
        "accessibility",
    }
    if set(bridges) != required_bridges:
        fail("host bridge inventory is incomplete")

    requirements = data.get("requirements", {})
    for key in (
        "foreign_runtime_cannot_access_host_kernel_directly",
        "foreign_isa_translation_cannot_bypass_runtime_isolation",
        "all_guest_to_host_resources_are_brokered",
        "semantic_accessibility_required_for_release",
        "runtime_crash_containment_required",
        "source_and_license_provenance_required",
    ):
        if requirements.get(key) is not True:
            fail(f"mandatory cross-runtime requirement disabled: {key}")

    gate = data.get("release_gate", {})
    if gate.get("status") != "ready":
        for key in (
            "vmm_verified",
            "arm64_translation_verified",
            "host_brokers_verified",
            "blind_user_integration_verified",
        ):
            if gate.get(key) is not False:
                fail(f"{key} cannot be true before cross-runtime status is ready")

    print("CROSS_RUNTIME_LOCK = PASS")
    print("VMM_BASELINE = PINNED")
    print("ARM64_TO_X86_64_TRANSLATION = PLANNED")
    print("HOST_BROKER_SET = COMPLETE_INVENTORY")


if __name__ == "__main__":
    main()
