#!/usr/bin/env python3
"""Fail-closed, bounded text patch engine for OMNI."""
from __future__ import annotations
import hashlib
import json
import os
import pathlib
import tempfile
from datetime import datetime, timezone

AGENT = pathlib.Path(__file__).resolve().parent
ROOT = AGENT.parent

def _load(name):
    return json.loads((AGENT / name).read_text(encoding="utf-8"))

def _sha256(data):
    return hashlib.sha256(data).hexdigest()

def _protected(path, policy):
    try:
        rel = path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return True
    return any(rel == p or rel.startswith(p.rstrip("/") + "/")
               for p in policy.get("protected_paths", []))

def _checkpoint(spec, before_sha, size):
    task_id = spec["task_id"]
    target = AGENT / "state" / "checkpoints"
    target.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema": 1,
        "task_id": task_id,
        "path": spec["path"],
        "expected_sha256": spec["expected_sha256"],
        "pre_sha256": before_sha,
        "pre_size": size,
        "mutation": "exact_text_replacement",
        "created": datetime.now(timezone.utc).isoformat(),
    }
    final = target / f"{task_id}.json"
    fd, tmp = tempfile.mkstemp(prefix=final.name + ".", dir=target)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            json.dump(payload, fh, indent=2)
            fh.write("\n")
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp, final)
    except Exception:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise

def apply_patch(spec):
    required = ("task_id", "path", "expected_sha256", "old_text", "new_text")
    if any(not spec.get(k) for k in required):
        return {"status": "BLOCKED", "error": "required patch field missing"}
    policy = _load("tool_policy.json")
    path = (ROOT / spec["path"]).resolve()
    if _protected(path, policy):
        return {"status": "BLOCKED", "error": "protected or outside path"}
    if not path.is_file():
        return {"status": "FAIL", "error": "target file does not exist"}
    max_bytes = int(_load("config.json").get("max_file_bytes", 10485760))
    raw = path.read_bytes()
    if len(raw) > max_bytes:
        return {"status": "BLOCKED", "error": "target exceeds maximum size"}
    before = _sha256(raw)
    if before != spec["expected_sha256"]:
        return {"status": "FAIL", "error": "expected SHA256 does not match current file", "actual_sha256": before}
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        return {"status": "BLOCKED", "error": "target is not UTF-8 text"}
    count = text.count(spec["old_text"])
    maximum = int(spec.get("max_replacements", 1))
    if count == 0:
        return {"status": "FAIL", "error": "old text not found"}
    if count > maximum or (spec.get("require_exact_replacements", True) and count != maximum):
        return {"status": "FAIL", "error": "replacement count guard failed", "matches": count}
    try:
        _checkpoint(spec, before, len(raw))
    except Exception as exc:
        return {"status": "UNKNOWN", "error": "checkpoint creation failed", "type": type(exc).__name__}
    updated = text.replace(spec["old_text"], spec["new_text"], maximum)
    data = updated.encode("utf-8")
    fd, tmp = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as fh:
            fh.write(data)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp, path)
    except Exception as exc:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        return {"status": "UNKNOWN", "error": "atomic write failed", "type": type(exc).__name__}
    after = _sha256(path.read_bytes())
    expected_after = _sha256(data)
    if after != expected_after:
        return {"status": "FAIL", "error": "post-write hash verification failed", "actual_sha256": after}
    return {"status": "PASS", "path": spec["path"], "pre_sha256": before, "post_sha256": after, "replacements": count}

if __name__ == "__main__":
    raise SystemExit(0)
