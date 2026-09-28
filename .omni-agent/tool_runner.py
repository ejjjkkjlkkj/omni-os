#!/usr/bin/env python3
"""Bounded local tool execution for OMNI."""
from __future__ import annotations
import json, pathlib, re, subprocess
from datetime import datetime, timezone
AGENT=pathlib.Path(__file__).resolve().parent
ROOT=AGENT.parent
def _load(p): return json.loads(p.read_text(encoding="utf-8"))
def _now(): return datetime.now(timezone.utc).isoformat()
def _scrub(s, words):
    for w in words:
        s=re.sub(r"(?i)("+re.escape(w)+r")\s*[:=]\s*[^\s,;]+", r"\1=[REDACTED]", s)
    return s
def _protected(path, policy):
    try: rel=(ROOT/pathlib.Path(path)).resolve().relative_to(ROOT.resolve()).as_posix()
    except Exception: return True
    return any(rel==p or rel.startswith(p.rstrip("/")+"/") for p in policy.get("protected_paths",[]))
def run_tool(tool_id,cwd=None,timeout=None):
    cat=_load(AGENT/"tools.json"); policy=_load(AGENT/"tool_policy.json")
    spec=next((x for x in cat["tools"] if x["id"]==tool_id),None)
    if not spec: return {"status":"UNKNOWN","tool":tool_id,"error":"tool not declared"}
    cmd=spec.get("command")
    if not cmd: return {"status":"UNKNOWN","tool":tool_id,"error":"tool has no executable command"}
    low=cmd.lower()
    if any(x.lower() in low for x in policy.get("blocked_patterns",[])): return {"status":"BLOCKED","tool":tool_id,"error":"blocked by policy"}
    if not any(cmd==x or cmd.startswith(x+" ") for x in policy.get("allowed_commands",[])): return {"status":"BLOCKED","tool":tool_id,"error":"not allowlisted"}
    wd=pathlib.Path(cwd or ROOT).resolve(); limit=int(timeout or policy.get("default_timeout_seconds",120)); started=_now()
    try:
        p=subprocess.run(cmd,cwd=wd,shell=True,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=limit,check=False)
        out=_scrub(p.stdout or "",policy.get("never_store",[])); cap=int(policy.get("max_output_bytes",262144))
        truncated=len(out.encode("utf-8","replace"))>cap
        if truncated: out=out.encode("utf-8","replace")[:cap].decode("utf-8","ignore")
        return {"status":"PASS" if p.returncode==0 else "FAIL","tool":tool_id,"command":cmd,"cwd":str(wd),"started":started,"finished":_now(),"exit_code":p.returncode,"output":out,"truncated":truncated}
    except subprocess.TimeoutExpired:
        return {"status":"UNKNOWN","tool":tool_id,"command":cmd,"cwd":str(wd),"started":started,"finished":_now(),"error":"timeout"}
    except Exception as e:
        return {"status":"UNKNOWN","tool":tool_id,"command":cmd,"cwd":str(wd),"started":started,"finished":_now(),"error":type(e).__name__}
def validate_write_path(path): return not _protected(path,_load(AGENT/"tool_policy.json"))
