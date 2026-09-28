#!/usr/bin/env python3
import json, pathlib, sys

if len(sys.argv) < 3:
    raise SystemExit("usage: coverage_gate.py <coverage-json> <path-substring>")
path = pathlib.Path(sys.argv[1])
needle = sys.argv[2].replace("\\", "/")
report = json.loads(path.read_text())
files = report["data"][0]["files"]
matches = [f for f in files if f["filename"].replace("\\", "/").endswith(needle)]
if len(matches) != 1:
    raise SystemExit(f"coverage target not unique: {needle!r}, matches={len(matches)}")
summary = matches[0]["summary"]
required = ("lines", "functions", "regions", "branches")
bad = {}
for key in required:
    percent = float(summary[key]["percent"])
    if percent != 100.0:
        bad[key] = percent
print({key: summary[key]["percent"] for key in required})
if bad:
    raise SystemExit(f"coverage below 100%: {bad}")
