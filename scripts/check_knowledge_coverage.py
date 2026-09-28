#!/usr/bin/env python3
"""Fail closed when repository technology, security, or accessibility coverage is incomplete."""
from __future__ import annotations
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
langs = json.loads((ROOT/"data/taxonomy/language-security-map.json").read_text(encoding="utf-8"))
network = json.loads((ROOT/"data/taxonomy/network-layers.json").read_text(encoding="utf-8"))
full = json.loads((ROOT/"data/taxonomy/full-stack-layers.json").read_text(encoding="utf-8"))
a11y = json.loads((ROOT/"data/taxonomy/accessibility-full-stack.json").read_text(encoding="utf-8"))

by_ext = {}
by_name = {}
for language, meta in langs["languages"].items():
    for marker in meta.get("extensions", []):
        if marker.startswith("."):
            by_ext[marker] = language
        else:
            by_name[marker] = language

special_suffixes = {".bpf.c": "eBPF"}
ignore_prefixes = (".git/", "data/threat-intel/generated/")
ignore_ext = {".md", ".txt", ".csv"}
unknown = set()
hits = set()

code_like = {
  ".c",".h",".cc",".cpp",".cxx",".hpp",".rs",".zig",".asm",".s",".S",".go",".py",".pyi",
  ".ps1",".psm1",".psd1",".sh",".bash",".cs",".csx",".java",".kt",".kts",".swift",".m",".mm",
  ".js",".mjs",".cjs",".jsx",".ts",".tsx",".mts",".cts",".php",".rb",".rake",".lua",".sql",
  ".sol",".wat",".wasm",".v",".sv",".svh",".vhd",".vhdl",".asl",".dsl",".tf",".tfvars",".hcl",
  ".yaml",".yml",".json",".xml",".xsd",".toml",".cmake",".mk",".groovy",".scala",".dart",".pl",
  ".pm",".jl",".erl",".hrl",".ex",".exs",".hs",".lhs",".ml",".mli",".move",".cu",".cuh",".cl",
  ".inf",".dsc",".dec",".fdf"
}

for path in ROOT.rglob("*"):
    if not path.is_file():
        continue
    rel = path.relative_to(ROOT).as_posix()
    if rel.startswith(ignore_prefixes):
        continue
    if path.name in by_name:
        hits.add(by_name[path.name]); continue
    special = next((lang for suffix,lang in special_suffixes.items() if path.name.endswith(suffix)), None)
    if special:
        hits.add(special); continue
    if path.suffix in by_ext:
        hits.add(by_ext[path.suffix]); continue
    if path.suffix in ignore_ext or path.name.startswith("."):
        continue
    if path.suffix in code_like:
        unknown.add(path.suffix)

required_publication = {
    "surface_web","deep_unindexed","deep_authenticated","closed_community","tor_onion","i2p",
    "zeronet","ipfs","hyphanet","gnunet","namecoin_bit","p2p_overlay","mixnet","mesh_overlay",
    "yggdrasil","cjdns_hyperboria","nym_mixnet","libp2p","other_overlay","cti_reporting","local_seclab"
}
network_publication=set(network["publication_layers"])
a11y_publication=set(a11y["publication_layers"])
missing_publication = sorted(required_publication - network_publication)
missing_a11y_publication = sorted(required_publication - a11y_publication)

layer_ids={x["id"] for x in full["layers"]}
required_depth={"L16","L15","L14","L13","L12","L11","L10","L9","L8","L7","L6","L5","L4","L3","L2","L1","L0","L-1","L-2","L-3","L-4","L-5","L-6"}
a11y_depth=set(a11y["stack_layers"])
missing_depth=sorted(required_depth-layer_ids)
missing_a11y_depth=sorted(required_depth-a11y_depth)

bad_a11y=[]
for layer_id, meta in a11y["stack_layers"].items():
    values=meta.get("accessibility") or []
    if not values:
        bad_a11y.append(layer_id)
for layer_id, meta in a11y["publication_layers"].items():
    values=meta.get("required") or []
    if not values:
        bad_a11y.append(f"publication:{layer_id}")

errors=False
if unknown:
    errors=True; print("ERROR unmapped code/config types:", ", ".join(sorted(unknown)))
if missing_publication:
    errors=True; print("ERROR missing publication/overlay layers:", ", ".join(missing_publication))
if missing_depth:
    errors=True; print("ERROR missing deep-stack layers:", ", ".join(missing_depth))
if missing_a11y_publication:
    errors=True; print("ERROR missing accessibility coverage for publication layers:", ", ".join(missing_a11y_publication))
if missing_a11y_depth:
    errors=True; print("ERROR missing accessibility coverage for deep-stack layers:", ", ".join(missing_a11y_depth))
if bad_a11y:
    errors=True; print("ERROR empty accessibility requirements:", ", ".join(sorted(bad_a11y)))

if errors:
    sys.exit(1)

print("PASS mapped technologies:", ", ".join(sorted(hits)) or "none detected")
print("PASS publication/overlay security layers:", len(network_publication))
print("PASS deep security stack:", len(layer_ids), "layers")
print("PASS publication/overlay accessibility coverage:", len(a11y_publication))
print("PASS deep accessibility stack:", len(a11y_depth), "layers")
