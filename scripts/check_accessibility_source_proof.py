#!/usr/bin/env python3
"""Verify that accessibility source coverage is concrete, not nominal."""
from __future__ import annotations
import json
import pathlib
import sys

ROOT=pathlib.Path(__file__).resolve().parents[1]
sources=json.loads((ROOT/"data/threat-intel/sources.json").read_text(encoding="utf-8"))["sources"]
a11y=json.loads((ROOT/"data/taxonomy/accessibility-full-stack.json").read_text(encoding="utf-8"))
official=json.loads((ROOT/"data/taxonomy/accessibility-official-source-proof.json").read_text(encoding="utf-8"))
overlay=json.loads((ROOT/"data/taxonomy/accessibility-overlay-source-proof.json").read_text(encoding="utf-8"))
stack=json.loads((ROOT/"data/taxonomy/accessibility-stack-source-proof.json").read_text(encoding="utf-8"))

errors=[]
by_id={}
for source in sources:
    sid=source.get("id")
    if not sid:
        errors.append("source missing id")
        continue
    if sid in by_id:
        errors.append(f"duplicate source id: {sid}")
    by_id[sid]=source

a11y_sources={sid:s for sid,s in by_id.items() if s.get("domain")=="accessibility"}
if not a11y_sources:
    errors.append("no first-class accessibility sources")

official_ids=set(official.get("sources",{}))
overlay_ids={sid for ids in overlay.get("layers",{}).values() for sid in ids}
proof_ids=official_ids|overlay_ids

# Every first-class accessibility source must be backed by a proof table.
for sid,source in sorted(a11y_sources.items()):
    for field in ("domain","layer","kind"):
        if not source.get(field):
            errors.append(f"{sid}: missing {field}")
    if source.get("mode")=="import-only":
        if sid not in overlay_ids:
            errors.append(f"{sid}: import-only source has no overlay proof")
    else:
        if not source.get("url"):
            errors.append(f"{sid}: public/reference source has no URL")
        if sid not in official_ids:
            errors.append(f"{sid}: public/reference source has no concrete official proof")

# Official proof must include concrete layers and proof items.
for sid,profile in official.get("sources",{}).items():
    if sid not in a11y_sources:
        errors.append(f"official proof references unknown accessibility source: {sid}")
    if not profile.get("layers"):
        errors.append(f"{sid}: proof has no stack layers")
    if not profile.get("proof"):
        errors.append(f"{sid}: proof has no evidence items")

# Every publication/overlay layer declared by the accessibility model must have
# at least one actual source and all listed IDs must exist.
declared_publication=set(a11y.get("publication_layers",{}))
proved_publication=set(overlay.get("layers",{}))
for layer in sorted(declared_publication-proved_publication):
    errors.append(f"publication layer has no source proof: {layer}")
for layer,ids in overlay.get("layers",{}).items():
    if not ids:
        errors.append(f"publication layer has empty source list: {layer}")
    for sid in ids:
        if sid not in a11y_sources:
            errors.append(f"{layer}: unknown/non-accessibility source {sid}")
        elif a11y_sources[sid].get("layer") != layer and layer!="surface_web":
            errors.append(f"{layer}: source {sid} declares layer {a11y_sources[sid].get('layer')}")

# Every L16..L-6 layer in the accessibility model must have at least one source.
declared_stack=set(a11y.get("stack_layers",{}))
proved_stack=set(stack.get("layers",{}))
for layer in sorted(declared_stack-proved_stack):
    errors.append(f"stack layer has no source proof: {layer}")
for layer,ids in stack.get("layers",{}).items():
    if not ids:
        errors.append(f"stack layer has empty source list: {layer}")
    for sid in ids:
        if sid not in a11y_sources:
            errors.append(f"{layer}: unknown/non-accessibility source {sid}")

# Detect decorative proof entries that are never tied to either a publication
# layer or a stack layer.
used=set(overlay_ids)
for ids in stack.get("layers",{}).values():
    used.update(ids)
for sid,profile in official.get("sources",{}).items():
    if profile.get("layers") and profile.get("proof"):
        used.add(sid)
for sid in sorted(proof_ids-used):
    errors.append(f"proof source is not used by any coverage layer: {sid}")

if errors:
    for error in errors:
        print("ERROR:",error)
    print(f"FAIL: {len(errors)} accessibility source coverage error(s)")
    sys.exit(1)

print(f"PASS: {len(a11y_sources)} first-class accessibility sources")
print(f"PASS: {len(declared_publication)} publication/overlay layers backed by sources")
print(f"PASS: {len(declared_stack)} full-stack layers backed by sources")
print("PASS: no accessibility source is accepted as a name-only placeholder")
