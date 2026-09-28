#!/usr/bin/env python3
"""Zero-dependency local-first OMNI repository maintenance agent."""
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
  "python": bool({"pyproject.toml","pytest.ini","setup.cfg","tox.ini"} & paths or any(p.startswith(("tests/","test/")) and p.endswith(".py") for p in paths)),
  "dotnet": any(p.endswith((".sln",".slnx",".csproj",".fsproj",".vbproj")) for p in paths),
  "security": any("security" in p.lower() or "audit" in p.lower() for p in paths),
  "accessibility": any(any(k in p.lower() for k in ("accessib","screenreader","screen-reader","nvda","uia")) for p in paths),
  "ci": any(p.startswith(".github/workflows/") for p in paths),
 }
def work_items(inv,features):
 paths=[x["path"] for x in inv]
 items=[]
 def add(priority,kind,title,paths=None,reason=""):
  items.append({"priority":priority,"kind":kind,"title":title,"paths":paths or [],"reason":reason})
 if features["python"]: add("P1","verify","Run Python test suite",[p for p in paths if p.endswith(".py")],"Python test infrastructure detected")
 if features["dotnet"]: add("P1","verify","Build and test .NET projects",[p for p in paths if p.endswith((".sln",".slnx",".csproj"))],".NET project detected")
 if features["security"]: add("P0","security","Review security controls",[p for p in paths if "security" in p.lower() or "audit" in p.lower()],"Security-related project surface detected")
 if features["accessibility"]: add("P0","accessibility","Review accessibility and screen-reader coverage",[p for p in paths if any(k in p.lower() for k in ("accessib","screenreader","screen-reader","nvda","uia"))],"Accessibility surface detected")
 markers=[]
 for p in paths:
  if p.endswith((".py",".cs",".cpp",".h",".hpp",".ps1",".md",".json")) and not p.startswith(".git/"):
   try:
    text=(ROOT/p).read_text(encoding="utf-8",errors="ignore")
    for marker in ("TODO","FIXME","XXX"):
     if marker in text: markers.append(p); break
   except OSError: pass
 if markers:add("P2","maintenance","Resolve explicit TODO/FIXME markers",markers,"Explicit maintenance markers detected")
 if not items:add("P2","coverage","Expand evidence coverage",[],"No specialized work surface detected")
 return items
def choose_tools(features):
 chosen=["git.status","git.log","repo.inventory"]
 if features["python"]: chosen.append("test.python")
 if features["dotnet"]: chosen.extend(["build.dotnet","test.dotnet"])
 if features["security"]: chosen.append("security.audit")
 if features["accessibility"]: chosen.append("accessibility.audit")
 return chosen
def next_actions(features,knowledge_complete,security_available,results):
 actions=[]
 if not security_available: actions.append({"priority":"P0","id":"restore-security-source","reason":"required security source unavailable"})
 if not knowledge_complete: actions.append({"priority":"P0","id":"complete-knowledge-base","reason":"canonical knowledge incomplete"})
 for tool,aid,reason in (("build.dotnet","fix-dotnet-build","build verification failed"),("test.dotnet","fix-dotnet-tests","test verification failed"),("test.python","fix-python-tests","Python verification failed"),("security.audit","resolve-security-gate","security gate not proven"),("accessibility.audit","resolve-accessibility-gate","accessibility gate not proven")):
  if results.get(tool,{}).get("status") in {"FAIL","UNKNOWN","BLOCKED"}: actions.append({"priority":"P1","id":aid,"reason":reason})
 if not actions: actions.append({"priority":"P2","id":"expand-coverage","reason":"no immediate blocker detected"})
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
  should_verify=args.verify or cfg.get("mode")=="continuous"
  if should_verify:
   for t in selected:
    if t=="repo.inventory": results[t]={"status":"PASS","files":len(inv)}
    elif t=="git.status": results[t]={"status":"PASS","output":git(["status","--short"],ROOT)}
    elif t=="git.log": results[t]={"status":"PASS","output":git(["log","-5","--oneline"],ROOT)}
    else: results[t]=run_tool(t)
  work=work_items(inv,features)
  actions=next_actions(features,ks["complete"],security.is_dir,results)
  executed=[k for k,v in results.items() if v.get("status") in {"PASS","FAIL","BLOCKED","UNKNOWN"}]
  verified=bool(should_verify and executed and not blockers and all(results[k]["status"]=="PASS" for k in executed))
  snapshot={"schema":4,"timestamp":started,"agent":{"mode":cfg["mode"],"dimensions":cfg["dimensions"],"autonomous_cycle":True},"detection":features,"selected_tools":selected,"work_queue":work,"repository":{"root":str(ROOT),"branch":git(["branch","--show-current"],ROOT),"commit":git(["rev-parse","HEAD"],ROOT),"status":git(["status","--short"],ROOT)},"security_source":sec,"knowledge":ks,"solution_inventory":inv,"tools":results,"next_actions":actions,"validation":{"verified":verified,"blockers":blockers,"reason":"Verification is evidence from executed declared tools; UNKNOWN and BLOCKED never become PASS."}}
  tmp=STATE/"latest.tmp"; tmp.write_text(json.dumps(snapshot,indent=2,ensure_ascii=False)+"\n",encoding="utf-8"); os.replace(tmp,STATE/"latest.json")
  stamp=started.replace(":","").replace("+00:00","Z"); (STATE/f"cycle-{stamp}.json").write_text(json.dumps(snapshot,indent=2,ensure_ascii=False)+"\n",encoding="utf-8")
  (STATE/"last-run.txt").write_text(started+"\n",encoding="utf-8")
  if args.once:return
  time.sleep(max(5,interval))
if __name__=="__main__": main()
