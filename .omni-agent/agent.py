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
def detect(inv):
 paths={x["path"] for x in inv}
 return {
  "python": bool({"pyproject.toml","pytest.ini","setup.cfg","tox.ini"} & paths or any(p.startswith("tests/") or p.startswith("test/") for p in paths if p.endswith(".py"))),
  "dotnet": any(p.endswith((".sln",".slnx",".csproj",".fsproj",".vbproj")) for p in paths),
  "security": any("security" in p.lower() or "audit" in p.lower() for p in paths),
  "accessibility": any(any(k in p.lower() for k in ("accessib","screenreader","screen-reader","nvda","uia")) for p in paths),
  "ci": any(p.startswith(".github/workflows/") for p in paths),
 }
def choose_tools(features):
 chosen=["git.status","git.log","repo.inventory"]
 if features["python"]: chosen.append("test.python")
 if features["dotnet"]: chosen.extend(["build.dotnet","test.dotnet"])
 if features["security"]: chosen.append("security.audit")
 if features["accessibility"]: chosen.append("accessibility.audit")
 return chosen
def next_actions(features,knowledge_complete,security_available,results):
 actions=[]
 if not security_available: actions.append({"priority":"P0","id":"restore-security-source","reason":"required security source is unavailable"})
 if not knowledge_complete: actions.append({"priority":"P0","id":"complete-knowledge-base","reason":"canonical knowledge files are incomplete"})
 if features["dotnet"] and results.get("build.dotnet",{}).get("status")=="FAIL": actions.append({"priority":"P1","id":"fix-dotnet-build","reason":"build verification failed"})
 if features["python"] and results.get("test.python",{}).get("status")=="FAIL": actions.append({"priority":"P1","id":"fix-python-tests","reason":"Python verification failed"})
 if features["security"] and results.get("security.audit",{}).get("status") in {"FAIL","UNKNOWN","BLOCKED"}: actions.append({"priority":"P1","id":"resolve-security-gate","reason":"security gate is not proven"})
 if features["accessibility"] and results.get("accessibility.audit",{}).get("status") in {"FAIL","UNKNOWN","BLOCKED"}: actions.append({"priority":"P1","id":"resolve-accessibility-gate","reason":"accessibility gate is not proven"})
 if not actions: actions.append({"priority":"P2","id":"expand-coverage","reason":"no immediate blocker detected; inspect uncovered requirements and evidence"})
 return actions
def main():
 ap=argparse.ArgumentParser(); ap.add_argument("--once",action="store_true"); ap.add_argument("--interval",type=int); ap.add_argument("--verify",action="store_true"); args=ap.parse_args()
 cfg=load(CONFIG); STATE.mkdir(parents=True,exist_ok=True); security=pathlib.Path(cfg["security_source"]["path"]); interval=args.interval or int(cfg["interval_seconds"])
 required=[KNOWLEDGE/x for x in ("schema.json","sources.json","requirements.json","coverage.json")]
 while True:
  started=utc(); inv=inventory(ROOT,int(cfg["max_file_bytes"])); features=detect(inv)
  ks={"path":str(KNOWLEDGE),"available":KNOWLEDGE.is_dir(),"required_files":{p.name:p.is_file() for p in required}}; ks["complete"]=ks["available"] and all(ks["required_files"].values())
  sec={"path":str(security),"available":security.is_dir()}
  if security.is_dir(): sec.update({"branch":git(["branch","--show-current"],security),"commit":git(["rev-parse","HEAD"],security),"status":git(["status","--short"],security),"inventory":inventory(security,int(cfg["max_file_bytes"]))})
  blockers=[]
  if cfg["security_source"].get("required") and not security.is_dir(): blockers.append("required security source unavailable")
  if not ks["complete"]: blockers.append("canonical knowledge base incomplete")
  selected=choose_tools(features); results={}
  if args.verify:
   for t in selected:
    if t=="repo.inventory": results[t]={"status":"PASS","files":len(inv)}
    elif t=="git.status": results[t]={"status":"PASS","output":git(["status","--short"],ROOT)}
    elif t=="git.log": results[t]={"status":"PASS","output":git(["log","-5","--oneline"],ROOT)}
    else: results[t]=run_tool(t)
  actions=next_actions(features,ks["complete"],security.is_dir,results)
  executed=[k for k,v in results.items() if v.get("status") in {"PASS","FAIL","BLOCKED","UNKNOWN"}]
  verified=bool(args.verify and executed and not blockers and all(results[k]["status"]=="PASS" for k in executed))
  snapshot={"schema":3,"timestamp":started,"agent":{"mode":cfg["mode"],"dimensions":cfg["dimensions"]},"detection":features,"selected_tools":selected,"repository":{"root":str(ROOT),"branch":git(["branch","--show-current"],ROOT),"commit":git(["rev-parse","HEAD"],ROOT),"status":git(["status","--short"],ROOT)},"security_source":sec,"knowledge":ks,"solution_inventory":inv,"tools":results,"next_actions":actions,"validation":{"verified":verified,"blockers":blockers,"reason":"PASS is evidence from an executed declared tool; UNKNOWN and BLOCKED never become PASS."}}
  tmp=STATE/"latest.tmp"; tmp.write_text(json.dumps(snapshot,indent=2,ensure_ascii=False)+"\n",encoding="utf-8"); os.replace(tmp,STATE/"latest.json"); (STATE/"last-run.txt").write_text(started+"\n",encoding="utf-8")
  if args.once:return
  time.sleep(max(5,interval))
if __name__=="__main__": main()
