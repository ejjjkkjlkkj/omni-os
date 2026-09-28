#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
from dataclasses import dataclass

USES_KEY_RE = r'(?:"uses"|\'uses\'|uses)'
CONTINUE_KEY_RE = r'(?:"continue-on-error"|\'continue-on-error\'|continue-on-error)'
USES_RE = re.compile(
    r"^\s*-?\s*" + USES_KEY_RE + r"\s*:\s*([^\s#]+)",
    re.IGNORECASE,
)
FLOW_USES_RE = re.compile(
    r"(?:^|[\[{,])\s*-?\s*" + USES_KEY_RE + r"\s*:",
    re.IGNORECASE,
)
CONTINUE_RE = re.compile(
    r"(?:^|[{,])\s*-?\s*" + CONTINUE_KEY_RE + r"\s*:\s*true(?:\s*[,}]|\s*$)",
    re.IGNORECASE,
)
EXPLICIT_POLICY_KEY_RE = re.compile(
    r"^\s*\?\s*(?:" + USES_KEY_RE + "|" + CONTINUE_KEY_RE + r")\s*$",
    re.IGNORECASE,
)
QUOTED_MAPPING_KEY_RE = re.compile(
    r'''^\s*-?\s*(?:"(?:[^"\\]|\\.)*"|'(?:[^']|'')*')\s*:'''
)
ALIAS_MAPPING_KEY_RE = re.compile(r"^\s*-?\s*\*[A-Za-z0-9_.-]+\s*:")
GENERIC_EXPLICIT_KEY_RE = re.compile(r"^\s*\?\s+\S")
COMMIT_REF_RE = re.compile(r"^[0-9a-fA-F]{40}$")
DOCKER_DIGEST_RE = re.compile(r"^docker://.+@sha256:[0-9a-fA-F]{64}$")
MUTABLE_RUNNER_RE = re.compile(r"(?:ubuntu|windows|macos)-latest", re.IGNORECASE)
PIP_INSTALL_RE = re.compile(r"\bpip(?:3(?:\.\d+)?)?\s+install\b", re.IGNORECASE)
BLOCK_SCALAR_HEADER_RE = re.compile(
    r":\s*[|>](?:[+-]?[1-9]?|[1-9][+-]?)?\s*$"
)


@dataclass(frozen=True)
class Violation:
    path: str
    line: int
    code: str
    value: str


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


def inspect_file(path: pathlib.Path) -> list[Violation]:
    violations: list[Violation] = []
    lines = path.read_text(encoding="utf-8").splitlines()
    block_scalar_parent_indent: int | None = None
    for index, raw in enumerate(lines):
        number = index + 1
        active = _strip_unquoted_comment(raw)
        stripped = active.strip()
        indent = len(active) - len(active.lstrip(" "))

        in_block_scalar = False
        if block_scalar_parent_indent is not None:
            if not stripped or indent > block_scalar_parent_indent:
                in_block_scalar = True
            else:
                block_scalar_parent_indent = None

        if not in_block_scalar and BLOCK_SCALAR_HEADER_RE.search(active):
            block_scalar_parent_indent = indent

        if in_block_scalar:
            if PIP_INSTALL_RE.search(active):
                command_parts = [stripped]
                cursor = index
                while command_parts[-1].rstrip().endswith("\\") and cursor + 1 < len(lines):
                    cursor += 1
                    command_parts.append(_strip_unquoted_comment(lines[cursor]).strip())
                command = " ".join(command_parts)
                if "--require-hashes" not in command:
                    violations.append(
                        Violation(str(path), number, "PIP_INSTALL_WITHOUT_HASHES", command)
                    )
            continue

        if (
            QUOTED_MAPPING_KEY_RE.search(active)
            or ALIAS_MAPPING_KEY_RE.search(active)
            or GENERIC_EXPLICIT_KEY_RE.search(active)
        ):
            violations.append(
                Violation(
                    str(path),
                    number,
                    "POLICY_KEY_SYNTAX_UNSUPPORTED",
                    active.strip(),
                )
            )
            continue

        if EXPLICIT_POLICY_KEY_RE.match(active):
            violations.append(
                Violation(
                    str(path),
                    number,
                    "POLICY_KEY_SYNTAX_UNSUPPORTED",
                    active.strip(),
                )
            )
            continue

        if CONTINUE_RE.search(active):
            violations.append(
                Violation(str(path), number, "CONTINUE_ON_ERROR_TRUE", active.strip())
            )
        if MUTABLE_RUNNER_RE.search(active):
            violations.append(
                Violation(str(path), number, "MUTABLE_RUNNER_LABEL", active.strip())
            )

        if PIP_INSTALL_RE.search(active):
            command_parts = [active.strip()]
            cursor = index
            while command_parts[-1].rstrip().endswith("\\") and cursor + 1 < len(lines):
                cursor += 1
                command_parts.append(_strip_unquoted_comment(lines[cursor]).strip())
            command = " ".join(command_parts)
            if "--require-hashes" not in command:
                violations.append(
                    Violation(str(path), number, "PIP_INSTALL_WITHOUT_HASHES", command)
                )

        match = USES_RE.match(active)
        if not match:
            if FLOW_USES_RE.search(active):
                violations.append(
                    Violation(str(path), number, "USES_SYNTAX_UNSUPPORTED", active.strip())
                )
            continue
        target = match.group(1)

        if target.startswith("./"):
            continue
        if target.startswith("docker://"):
            if not DOCKER_DIGEST_RE.fullmatch(target):
                violations.append(
                    Violation(str(path), number, "DOCKER_NOT_DIGEST_PINNED", target)
                )
            continue
        if "@" not in target:
            violations.append(Violation(str(path), number, "ACTION_REF_MISSING", target))
            continue

        action, ref = target.rsplit("@", 1)
        if "/" not in action or not COMMIT_REF_RE.fullmatch(ref):
            violations.append(
                Violation(str(path), number, "ACTION_NOT_COMMIT_PINNED", target)
            )

    return violations


def inspect(root: pathlib.Path) -> list[Violation]:
    if not root.is_dir():
        raise ValueError(f"workflow directory does not exist: {root}")
    violations: list[Violation] = []
    for path in sorted(root.glob("*.y*ml")):
        violations.extend(inspect_file(path))
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description="Fail-closed GitHub Actions policy")
    parser.add_argument("root", nargs="?", default=".github/workflows", type=pathlib.Path)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    try:
        violations = inspect(args.root)
    except (OSError, ValueError) as exc:
        print(json.dumps({"status": "FAIL", "error": str(exc)}, sort_keys=True))
        return 2

    result = {
        "schema": "omni.workflow-policy.v4",
        "status": "PASS" if not violations else "FAIL",
        "violations": [v.__dict__ for v in violations],
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return 1 if args.strict and violations else 0


if __name__ == "__main__":
    raise SystemExit(main())
