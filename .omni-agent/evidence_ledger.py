#!/usr/bin/env python3
"""Append-only, secret-scrubbed evidence records for OMNI."""
from __future__ import annotations
import hashlib
import json
import pathlib
import re
from datetime import datetime, timezone

AGENT = pathlib.Path(__file__).resolve().parent
ROOT = AGENT.parent
LEDGER = AGENT / "state" / "evidence.jsonl"

SECRET_KEYS = ("password", "token", "api_key", "private_key", "secret", "recovery_key", "pin")

def _scrub(value):
    text = str(value)
    for key in SECRET_KEYS:
        text = re.sub(r"(?i)" + re.escape(key) + r"\s*[:=]\s*[^\s,;]+", key + "=[REDACTED]", text)
    return text

def record(kind, status, subject, details=None):
    if status not in {"PASS", "FAIL", "UNKNOWN", "BLOCKED", "ENVIRONMENT", "REGRESSION", "PROVEN", "RELEASED"}:
        return {"status": "BLOCKED", "error": "invalid evidence status"}
    payload = {
        "schema": 1,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "kind": _scrub(kind),
        "status": status,
        "subject": _scrub(subject),
        "details": _scrub(details or {}),
    }
    raw = json.dumps(payload, ensure_ascii=False, sort_keys=True)
    digest = hashlib.sha256(raw.encode("utf-8")).hexdigest()
    payload["record_sha256"] = digest
    LEDGER.parent.mkdir(parents=True, exist_ok=True)
    with LEDGER.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n")
    return {"status": "PASS", "record_sha256": digest}

if __name__ == "__main__":
    raise SystemExit(0)
