#!/usr/bin/env python3
"""Zero-dependency local-first OMNI repository agent.

The runtime is deliberately stdlib-only. It inventories the solution and the
local omni-security repository, builds a deterministic state snapshot, and
records evidence. It never claims validation without evidence and never
silently deletes or rewrites repository history.
"""
from __future__ import annotations
import argparse, hashlib, json, os, pathlib, subprocess, time
from datetime import datetime, timezone

ROOT = pathlib.Path(__file__).resolve().parents[1]
AGENT = ROOT / ".omni-agent"
STATE = AGENT / "state"
KNOWLEDGE = AGENT / "knowledge"
CONFIG = AGENT / "config.json"

def utc():
    return datetime.now(timezone.utc).isoformat()

def load_config():
    return json.loads(CONFIG.read_text(encoding="utf-8"))

def sha256_file(p):
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def git(args, cwd):
    try:
        return subprocess.run(["git", *args], cwd=cwd, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              timeout=30, check=False).stdout.strip()
    except Exception as e:
        return f"ERROR: {e}"

def inventory(root, max_bytes):
    rows = []
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in {".git", "__pycache__", ".venv", "node_modules"}]
        for name in files:
            p = pathlib.Path(base) / name
            try:
                size = p.stat().st_size
                if size <= max_bytes:
                    rows.append({
                        "path": p.relative_to(root).as_posix(),
                        "size": size,
                        "sha256": sha256_file(p)
                    })
            except (OSError, PermissionError):
                continue
    return sorted(rows, key=lambda x: x["path"])

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--once", action="store_true")
    ap.add_argument("--interval", type=int)
    args = ap.parse_args()
    cfg = load_config()
    STATE.mkdir(parents=True, exist_ok=True)
    security = pathlib.Path(cfg["security_source"]["path"])
    interval = args.interval or int(cfg["interval_seconds"])
    required_knowledge = [KNOWLEDGE / "schema.json", KNOWLEDGE / "sources.json", KNOWLEDGE / "requirements.json", KNOWLEDGE / "coverage.json"]

    while True:
        started = utc()
        repo = {
            "root": str(ROOT),
            "branch": git(["branch", "--show-current"], ROOT),
            "commit": git(["rev-parse", "HEAD"], ROOT),
            "status": git(["status", "--short"], ROOT),
        }
        knowledge_state = {"path": str(KNOWLEDGE), "available": KNOWLEDGE.is_dir(), "required_files": {p.name: p.is_file() for p in required_knowledge}}
        knowledge_state["complete"] = knowledge_state["available"] and all(knowledge_state["required_files"].values())
        security_state = {"path": str(security), "available": security.is_dir()}
        if security.is_dir():
            security_state.update({
                "branch": git(["branch", "--show-current"], security),
                "commit": git(["rev-parse", "HEAD"], security),
                "status": git(["status", "--short"], security),
                "inventory": inventory(security, int(cfg["max_file_bytes"]))
            })

        blockers = []
        if cfg["security_source"].get("required") and not security.is_dir():
            blockers.append("required security source unavailable")
        if not knowledge_state["complete"]:
            blockers.append("canonical knowledge base incomplete")
        snapshot = {
            "schema": 1,
            "timestamp": started,
            "agent": {"mode": cfg["mode"], "dimensions": cfg["dimensions"]},
            "repository": repo,
            "security_source": security_state,
            "knowledge": knowledge_state,
            "solution_inventory": inventory(ROOT, int(cfg["max_file_bytes"])),
            "validation": {
                "verified": False,
                "blockers": blockers,
                "reason": "Inventory/evidence collection only; no build/test result asserted."
            }
        }
        target = STATE / "latest.json"
        tmp = target.with_suffix(".tmp")
        tmp.write_text(json.dumps(snapshot, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        os.replace(tmp, target)
        (STATE / "last-run.txt").write_text(started + "\n", encoding="utf-8")
        if args.once:
            return
        time.sleep(max(5, interval))

if __name__ == "__main__":
    main()
