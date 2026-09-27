#!/usr/bin/env python3
"""Fail closed when repository technologies are not covered by OMNI taxonomies."""
from __future__ import annotations
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
langs = json.loads((ROOT/"data/taxonomy/language-security-map.json").read_text(encoding="utf-8"))
layers = json.loads((ROOT/"data/taxonomy/network-layers.json").read_text(encoding="utf-8"))

mapped_ext = {}
for lang, meta in langs["languages"].items():
    for ext in meta.get("extensions", []):
        mapped_ext[ext] = lang

CODE_EXTENSIONS = {
    ".c",".h",".cc",".cpp",".cxx",".hpp",".hh",".hxx",".rs",".zig",".asm",".s",".S",
    ".go",".py",".pyi",".ps1",".psm1",".psd1",".sh",".bash",".cs",".csx",".java",
    ".kt",".kts",".swift",".m",".mm",".js",".mjs",".cjs",".jsx",".ts",".tsx",".mts",
    ".cts",".php",".rb",".rake",".lua",".sql",".sol",".wat",".wasm"
}
unknown = set()
seen = set()
for p in ROOT.rglob("*"):
    if not p.is_file():
        continue
    rel = p.relative_to(ROOT).as_posix()
    if rel.startswith(".git/") or rel.startswith("data/threat-intel/generated/"):
        continue
    ext = p.suffix
    if ext in mapped_ext:
        seen.add(mapped_ext[ext])
    elif ext in CODE_EXTENSIONS:
        unknown.add(ext)

required_publication = {
    "surface_web","deep_unindexed","deep_authenticated","closed_community","tor_onion",
    "i2p","zeronet","ipfs","hyphanet","gnunet","namecoin_bit","p2p_overlay","mixnet",
    "mesh_overlay","other_overlay","cti_reporting","local_seclab"
}
missing_layers = sorted(required_publication - set(layers["publication_layers"]))

if unknown or missing_layers:
    if unknown:
        print("ERROR unmapped code extensions:", ", ".join(sorted(unknown)))
    if missing_layers:
        print("ERROR missing network layers:", ", ".join(missing_layers))
    sys.exit(1)

print("PASS language coverage:", ", ".join(sorted(seen)) or "no mapped source language detected")
print("PASS network/overlay taxonomy:", len(layers["publication_layers"]), "publication layers")
