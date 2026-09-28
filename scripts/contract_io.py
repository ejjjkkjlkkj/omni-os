#!/usr/bin/env python3
"""Fail-closed loading of required repository-contract inputs.

A missing, empty, or malformed required input is a fail-closed MISSING/INVALID
state, never a silent success and never an uncaught traceback. The loader prints
a precise diagnostic and exits non-zero so the surrounding gate stays red with a
legible reason instead of crashing.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


def require_json(path: str | Path, *, kind: str = "input") -> Any:
    """Return parsed JSON from ``path`` or fail closed (exit 1) with a reason."""
    p = Path(path)
    if not p.is_file() or p.stat().st_size == 0:
        print(f"MISSING: required {kind} is absent or empty: {p}")
        raise SystemExit(1)
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"INVALID: required {kind} is not valid JSON: {p}: {exc}")
        raise SystemExit(1)
