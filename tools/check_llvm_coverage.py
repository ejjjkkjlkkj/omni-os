#!/usr/bin/env python3
import json, subprocess, sys
if len(sys.argv) != 3:
    raise SystemExit("usage: check_llvm_coverage.py <binary> <profdata>")
binary, profdata = sys.argv[1:]
raw=subprocess.check_output(["llvm-cov","export",binary,f"-instr-profile={profdata}"],text=True)
totals=json.loads(raw)["data"][0]["totals"]
vals={k:totals[k]["percent"] for k in ("lines","branches","functions")}
print(" ".join(f"{k}={v:.2f}%" for k,v in vals.items()))
if min(vals.values()) < 100.0: raise SystemExit("critical native coverage is not 100%")
