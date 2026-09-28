#!/usr/bin/env python3
"""Zero-dependency local-first OMNI repository agent."""
from __future__ import annotations
import argparse,hashlib,json,os,pathlib,subprocess,time
from datetime import datetime,timezone
ROOT=pathlib.Path(__file__).resolve().parents[1]; AGENT=ROOT/".omni-agent"; STATE=AGENT/"state"; KNOWLEDGE=AGENT/"knowledge"; CONFIG=AGENT/"config.json"
def utc(): return datetime.now(timezone.utc).isoformat()
def load(p): return json.loads(p.read_text(encoding="utf-8"))
def sha(p):
 h=hashlib.sha256()
 with p.open("rb") as f:
  for c in iter(lambda:f.read(1024*1024),b""): h.update(c)
 return h.hexdigest()
def git(args,cwd):
 try:return subprocess.run(["git",*args],cwd=cwd,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=30,check=False).stdout.strip()
 except Exception as e:return "ERROR: "+type(e).__name__
def inventory(root,max_bytes):
 rows=[]
 for base,dirs,files in os.walk(root):
  dirs[:]=[d for d in dirs if d not in {".git","__pycache__",".venv","node_modules"}]
  for n in files:
   p=pathlib.Path(base)/n
   try:
    if p.stat().st_size<=max_bytes: rows.append({"path":p.relative_to(root).as_posix(),"size":p.stat().st_size,"sha256":sha(p)})
   except (OSError,PermissionError): pass
 return sorted(rows,key=lambda x:x["path"])
def run_tool(tool_id,cwd=None):
 from tool_runner import run_tool as execute
 return execute(tool_id,cwd=cwd)
def tool_probe(cfg):
 results={}
 for tool in ("git.status","git.log","repo.inventory","test.python","test.dotnet","build.dotnet","security.audit","accessibility.audit"):
  if tool=="repo.inventory": results[tool]={"status":"PASS","files":len(inventory(ROOT,int(cfg["max_file_bytes"])))}
  elif tool in {"git.status","git.log"}: results[tool]={"status":"PASS","output":git(["status","--short"] if tool=="git.status" else ["log","-5","--oneline"],ROOT)}
  else: results[tool]=run_tool(tool)
 return results
def main():
 ap=argparse.ArgumentParser(); ap.add_argument("--once",action="store_true"); ap.add_argument("--interval",type=int); ap.add_argument("--verify",action="store_true"); args=ap.parse_args()
 cfg=load(CONFIG); STATE.mkdir(parents=True,exist_ok=True); security=pathlib.Path(cfg["security_source"]["path"]); interval=args.interval or int(cfg["interval_seconds"])
 required=[KNOWLEDGE/x for x in ("schema.json","sources.json","requirements.json","coverage.json")]
 while True:
  started=utc(); ks={"path":str(KNOWLEDGE),"available":KNOWLEDGE.is_dir(),"required_files":{p.name:p.is_file() for p in required}}; ks["complete"]=ks["available"] and all(ks["required_files"].values())
  sec={"path":str(security),"available":security.is_dir()}
  if security.is_dir(): sec.update({"branch":git(["branch","--show-current"],security),"commit":git(["rev-parse","HEAD"],security),"status":git(["status","--short"],security),"inventory":inventory(security,int(cfg["max_file_bytes"]))})
  blockers=[]
  if cfg["security_source"].get("required") and not security.is_dir(): blockers.append("required security source unavailable")
  if not ks["complete"]: blockers.append("canonical knowledge base incomplete")
  tools=tool_probe(cfg) if args.verify else {}
  verified=args.verify and not any(v.get("status") in {"FAIL","BLOCKED","UNKNOWN"} for v in tools.values() if isinstance(v,dict))
  snapshot={"schema":2,"timestamp":started,"agent":{"mode":cfg["mode"],"dimensions":cfg["dimensions"]},"repository":{"root":str(ROOT),"branch":git(["branch","--show-current"],ROOT),"commit":git(["rev-parse","HEAD"],ROOT),"status":git(["status","--short"],ROOT)},"security_source":sec,"knowledge":ks,"solution_inventory":inventory(ROOT,int(cfg["max_file_bytes"])),"tools":tools,"validation":{"verified":verified,"blockers":blockers,"reason":"Verification is asserted only when --verify ran and all executed tools returned no failure/block/unknown."}}
  tmp=STATE/"latest.tmp"; tmp.write_text(json.dumps(snapshot,indent=2,ensure_ascii=False)+"\n",encoding="utf-8"); os.replace(tmp,STATE/"latest.json"); (STATE/"last-run.txt").write_text(started+"\n",encoding="utf-8")
  if args.once:return
  time.sleep(max(5,interval))
if __name__=="__main__": main()
