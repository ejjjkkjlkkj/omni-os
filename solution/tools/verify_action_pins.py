#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
USES_KEY_RE = r'(?:"uses"|\'uses\'|uses)'
USES_RE = re.compile(
    r"^\s*(?:-\s*)?" + USES_KEY_RE + r"\s*:\s*([^\s#]+)\s*(?:#.*)?$",
    re.IGNORECASE,
)
FLOW_USES_RE = re.compile(
    r"(?:^|[\[{,])\s*-?\s*" + USES_KEY_RE + r"\s*:",
    re.IGNORECASE,
)
EXPLICIT_USES_KEY_RE = re.compile(
    r"^\s*\?\s*" + USES_KEY_RE + r"\s*$",
    re.IGNORECASE,
)
QUOTED_MAPPING_KEY_RE = re.compile(
    r'''^\s*-?\s*(?:"(?:[^"\\]|\\.)*"|'(?:[^']|'')*')\s*:'''
)
ALIAS_MAPPING_KEY_RE = re.compile(r"^\s*-?\s*\*[A-Za-z0-9_.-]+\s*:")
GENERIC_EXPLICIT_KEY_RE = re.compile(r"^\s*\?\s+\S")
ACTION_REF_RE = re.compile(
    r"^([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*)@([0-9a-fA-F]{40})$"
)
DOCKER_DIGEST_RE = re.compile(r"^docker://[^@\s]+@sha256:[0-9a-fA-F]{64}$")
SHA40_RE = re.compile(r"^[0-9a-fA-F]{40}$")
ACTION_LOCK_KEYS = {
    "actions/checkout": "checkout",
    "actions/setup-python": "setup_python",
    "actions/upload-artifact": "upload_artifact",
    "actions/download-artifact": "download_artifact",
    "actions/attest": "attest",
}


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


def _locked_actions(root: pathlib.Path) -> dict[str, str]:
    lock_path = root / "toolchains.lock.json"
    try:
        lock = json.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read toolchain lock: {exc}") from exc

    raw = lock.get("github_actions")
    if not isinstance(raw, dict):
        raise ValueError("toolchain lock is missing github_actions mapping")

    locked: dict[str, str] = {}
    for action, key in ACTION_LOCK_KEYS.items():
        if key not in raw:
            continue
        sha = raw[key]
        if not isinstance(sha, str) or not SHA40_RE.fullmatch(sha):
            raise ValueError(f"invalid SHA for github_actions.{key}")
        locked[action] = sha.lower()
    return locked


def verify(root: pathlib.Path = ROOT) -> dict[str, object]:
    workflows = root / ".github" / "workflows"
    violations: list[dict[str, str]] = []
    checked = 0

    try:
        locked_actions = _locked_actions(root)
    except ValueError as exc:
        return {
            "schema": "omniexec.action-pins.v5",
            "status": "FAIL",
            "checked": 0,
            "violations": [
                {
                    "file": "toolchains.lock.json",
                    "line": "0",
                    "uses": "",
                    "reason": f"INVALID_LOCK: {exc}",
                }
            ],
        }

    for path in sorted(workflows.glob("*.y*ml")):
        relative = path.relative_to(root).as_posix()
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            active = _strip_unquoted_comment(line)
            if (
                QUOTED_MAPPING_KEY_RE.search(active)
                or ALIAS_MAPPING_KEY_RE.search(active)
                or GENERIC_EXPLICIT_KEY_RE.search(active)
            ):
                checked += 1
                violations.append(
                    {
                        "file": relative,
                        "line": str(line_number),
                        "uses": active.strip(),
                        "reason": "USES_SYNTAX_UNSUPPORTED",
                    }
                )
                continue

            if EXPLICIT_USES_KEY_RE.match(active):
                checked += 1
                violations.append(
                    {
                        "file": relative,
                        "line": str(line_number),
                        "uses": active.strip(),
                        "reason": "USES_SYNTAX_UNSUPPORTED",
                    }
                )
                continue

            match = USES_RE.match(active)
            if not match:
                if FLOW_USES_RE.search(active):
                    checked += 1
                    violations.append(
                        {
                            "file": relative,
                            "line": str(line_number),
                            "uses": active.strip(),
                            "reason": "USES_SYNTAX_UNSUPPORTED",
                        }
                    )
                continue

            value = match.group(1)
            checked += 1
            if value.startswith("./"):
                continue

            if value.startswith("docker://"):
                if not DOCKER_DIGEST_RE.fullmatch(value):
                    violations.append(
                        {
                            "file": relative,
                            "line": str(line_number),
                            "uses": value,
                            "reason": "DOCKER_NOT_DIGEST_PINNED",
                        }
                    )
                continue

            action_match = ACTION_REF_RE.fullmatch(value)
            if not action_match:
                violations.append(
                    {
                        "file": relative,
                        "line": str(line_number),
                        "uses": value,
                        "reason": "ACTION_NOT_SHA_PINNED",
                    }
                )
                continue

            action, sha = action_match.groups()
            expected = locked_actions.get(action)
            if expected is None:
                violations.append(
                    {
                        "file": relative,
                        "line": str(line_number),
                        "uses": value,
                        "reason": "ACTION_NOT_LOCKED",
                    }
                )
            elif sha.lower() != expected:
                violations.append(
                    {
                        "file": relative,
                        "line": str(line_number),
                        "uses": value,
                        "reason": "ACTION_LOCK_MISMATCH",
                    }
                )

    return {
        "schema": "omniexec.action-pins.v5",
        "status": "PASS" if not violations else "FAIL",
        "checked": checked,
        "violations": violations,
    }


def main() -> int:
    result = verify()
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
