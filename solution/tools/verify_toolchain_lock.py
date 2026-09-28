#!/usr/bin/env python3
from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
HEX40_RE = re.compile(r"^[0-9a-fA-F]{40}$")
HEX64_RE = re.compile(r"^[0-9a-fA-F]{64}$")


def _string(lock: dict[str, Any], section: str, key: str) -> str:
    value = lock.get(section, {}).get(key) if isinstance(lock.get(section), dict) else None
    if not isinstance(value, str) or not value:
        raise ValueError(f"missing or invalid {section}.{key}")
    return value


def _validate_lock(lock: dict[str, Any]) -> list[dict[str, str]]:
    invalid: list[dict[str, str]] = []
    hex40 = [
        ("edk2_primary", "commit"),
        ("sct", "edk2_commit"),
        ("sct", "commit"),
        ("formal", "opam_repository_sha"),
        ("qemu", "release_key_fingerprint"),
    ]
    hex64 = [
        ("cbmc", "ubuntu_24_04_deb_sha256"),
        ("qemu", "tarball_sha256"),
        ("actionlint", "linux_amd64_sha256"),
    ]
    for section, key in hex40:
        try:
            value = _string(lock, section, key)
        except ValueError as exc:
            invalid.append({"field": f"{section}.{key}", "value": str(exc)})
            continue
        if not HEX40_RE.fullmatch(value):
            invalid.append({"field": f"{section}.{key}", "value": value})
    for section, key in hex64:
        try:
            value = _string(lock, section, key)
        except ValueError as exc:
            invalid.append({"field": f"{section}.{key}", "value": str(exc)})
            continue
        if not HEX64_RE.fullmatch(value):
            invalid.append({"field": f"{section}.{key}", "value": value})

    try:
        slirp_mode = _string(lock, "qemu", "slirp_mode")
        slirp_package = _string(lock, "qemu", "slirp_package")
    except ValueError as exc:
        invalid.append({"field": "qemu.slirp", "value": str(exc)})
    else:
        if slirp_mode != "system":
            invalid.append({"field": "qemu.slirp_mode", "value": slirp_mode})
        if slirp_package != "libslirp-dev":
            invalid.append({"field": "qemu.slirp_package", "value": slirp_package})

    actions = lock.get("github_actions")
    if not isinstance(actions, dict) or not actions:
        invalid.append({"field": "github_actions", "value": "missing or invalid mapping"})
    else:
        for key, value in sorted(actions.items()):
            if not isinstance(value, str) or not HEX40_RE.fullmatch(value):
                invalid.append({"field": f"github_actions.{key}", "value": str(value)})
    return invalid


def _strip_unquoted_comment(line: str) -> str:
    quote: str | None = None
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
            continue
        if char == "\\" and quote == '"':
            escaped = True
            continue
        if char in {"'", '"'}:
            if quote is None:
                quote = char
            elif quote == char:
                quote = None
            continue
        if char == "#" and quote is None:
            return line[:index]
    return line


def _active_text(text: str) -> str:
    lines = []
    for raw in text.splitlines():
        active = _strip_unquoted_comment(raw).strip()
        if active:
            lines.append(active)
    return "\n".join(lines)


def verify(root: Path = ROOT) -> dict[str, object]:
    try:
        raw = json.loads((root / "toolchains.lock.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return {
            "schema": "omniexec.toolchain-lock-verification.v4",
            "status": "FAIL",
            "missing": [],
            "invalid": [{"field": "toolchains.lock.json", "value": str(exc)}],
            "checked_files": [],
        }
    if not isinstance(raw, dict):
        return {
            "schema": "omniexec.toolchain-lock-verification.v4",
            "status": "FAIL",
            "missing": [],
            "invalid": [{"field": "toolchains.lock.json", "value": "root must be an object"}],
            "checked_files": [],
        }
    lock: dict[str, Any] = raw
    invalid = _validate_lock(lock)
    if invalid:
        return {
            "schema": "omniexec.toolchain-lock-verification.v4",
            "status": "FAIL",
            "missing": [],
            "invalid": invalid,
            "checked_files": [],
        }

    primary_tag = _string(lock, "edk2_primary", "tag")
    primary_commit = _string(lock, "edk2_primary", "commit")
    sct_edk2_tag = _string(lock, "sct", "edk2_tag")
    sct_edk2_commit = _string(lock, "sct", "edk2_commit")
    sct_tag = _string(lock, "sct", "tag")
    sct_commit = _string(lock, "sct", "commit")
    cbmc_version = _string(lock, "cbmc", "version")
    cbmc_hash = _string(lock, "cbmc", "ubuntu_24_04_deb_sha256")
    qemu_slirp_mode = _string(lock, "qemu", "slirp_mode")
    qemu_slirp_package = _string(lock, "qemu", "slirp_package")
    actionlint_version = _string(lock, "actionlint", "version")
    actionlint_hash = _string(lock, "actionlint", "linux_amd64_sha256")

    checks: dict[str, list[str]] = {
        ".github/workflows/workflow-lint.yml": [
            f"https://github.com/rhysd/actionlint/releases/download/v{actionlint_version}/actionlint_{actionlint_version}_linux_amd64.tar.gz",
            f"{actionlint_hash}  actionlint.tar.gz",
        ],
        ".github/workflows/formal-semantic-proof.yml": [
            f"gh release download cbmc-{cbmc_version}",
            f"{cbmc_hash}  ubuntu-24.04-cbmc-{cbmc_version}-Linux.deb",
            "run: bash scripts/install_pinned_framac.sh",
        ],
        ".github/workflows/deep-software-ceiling.yml": [
            f"git clone --depth 1 --branch {primary_tag} --recurse-submodules https://github.com/tianocore/edk2.git",
            f'test "$(git -C edk2 rev-parse HEAD)" = "{primary_commit}"',
            f"gh release download cbmc-{cbmc_version}",
            f"{cbmc_hash}  ubuntu-24.04-cbmc-{cbmc_version}-Linux.deb",
            "run: bash scripts/install_pinned_framac.sh",
            f"{qemu_slirp_package}",
            "run: bash scripts/build_pinned_qemu.sh",
        ],
        ".github/workflows/deep-software-gates.yml": [
            f"{qemu_slirp_package}",
            "run: bash scripts/build_pinned_qemu.sh",
        ],
        ".github/workflows/independent-verification.yml": [
            f"{qemu_slirp_package}",
            "run: bash scripts/build_pinned_qemu.sh",
        ],
        ".github/workflows/reproducibility.yml": [
            f"EDK2_TAG: {primary_tag}",
            f"EDK2_SHA: {primary_commit}",
        ],
        ".github/workflows/hardware-hil.yml": [
            f"git clone --depth 1 --branch {primary_tag} --recurse-submodules https://github.com/tianocore/edk2.git",
            f'test "$(git -C edk2 rev-parse HEAD)" = "{primary_commit}"',
            f"{qemu_slirp_package}",
            "bash scripts/build_pinned_qemu.sh",
        ],
        ".github/workflows/uefi-sct-build.yml": [
            f"git clone --depth 1 --branch {sct_edk2_tag} --recurse-submodules https://github.com/tianocore/edk2.git",
            f'test "$(git -C edk2 rev-parse HEAD)" = "{sct_edk2_commit}"',
            f"git clone --depth 1 --branch {sct_tag} https://github.com/tianocore/edk2-test.git",
            f'test "$(git -C edk2-test rev-parse HEAD)" = "{sct_commit}"',
        ],
        ".github/workflows/uefi-sct-runtime.yml": [
            f"EDK2_TAG: {sct_edk2_tag}",
            f"EDK2_SHA: {sct_edk2_commit}",
            f"SCT_TAG: {sct_tag}",
            f"SCT_SHA: {sct_commit}",
            f"{qemu_slirp_package}",
            "run: bash scripts/build_pinned_qemu.sh",
        ],
        "scripts/install_pinned_framac.sh": [
            f'OPAM_REPOSITORY_SHA="{_string(lock, "formal", "opam_repository_sha")}"',
            f'OCAML_PACKAGE="{_string(lock, "formal", "ocaml_package")}"',
            f'FRAMAC_PACKAGE="{_string(lock, "formal", "frama_c_package")}"',
            f'ALT_ERGO_PACKAGE="{_string(lock, "formal", "alt_ergo_package")}"',
        ],
        "scripts/build_pinned_qemu.sh": [
            f'QEMU_VERSION="{_string(lock, "qemu", "version")}"',
            f'QEMU_RELEASE_KEY_FPR="{_string(lock, "qemu", "release_key_fingerprint")}"',
            f'QEMU_TARBALL_SHA256="{_string(lock, "qemu", "tarball_sha256")}"',
            f'QEMU_SLIRP_MODE="{qemu_slirp_mode}"',
            f'QEMU_SLIRP_PACKAGE="{qemu_slirp_package}"',
            "--enable-slirp",
            "-netdev help",
        ],
    }

    missing: list[dict[str, str]] = []
    for relative, needles in checks.items():
        path = root / relative
        if not path.is_file():
            missing.append({"file": relative, "value": "<file missing>"})
            continue
        active = _active_text(path.read_text(encoding="utf-8"))
        for needle in needles:
            if needle not in active:
                missing.append({"file": relative, "value": needle})

    return {
        "schema": "omniexec.toolchain-lock-verification.v4",
        "status": "PASS" if not missing else "FAIL",
        "missing": missing,
        "invalid": [],
        "checked_files": sorted(checks),
    }


def main() -> int:
    result = verify()
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
