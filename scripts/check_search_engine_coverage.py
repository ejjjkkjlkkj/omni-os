#!/usr/bin/env python3
"""Fail-closed validation for OMNI's multi-engine search coverage map."""
from __future__ import annotations

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]

def load(rel: str):
    return json.loads((ROOT / rel).read_text(encoding="utf-8"))

sources = load("data/threat-intel/sources.json").get("sources", [])
source_by_id = {s.get("id"): s for s in sources if s.get("id")}
taxonomy = load("data/taxonomy/search-engine-coverage.json")
engines = taxonomy.get("engines")

errors: list[str] = []
required_layers = {"surface_web", "deep_unindexed", "tor_onion"}
allowed_categories = {"general", "privacy", "independent", "metasearch", "dark_web_index", "scholarly", "scholarly_deep"}

if not isinstance(engines, list) or not engines:
    errors.append("search-engine taxonomy must contain a non-empty engines array")
else:
    ids = [e.get("id") for e in engines if isinstance(e, dict)]
    if len(ids) != len(set(ids)):
        errors.append("search-engine taxonomy contains duplicate IDs")
    for entry in engines:
        if not isinstance(entry, dict):
            errors.append("search-engine taxonomy contains a non-object entry")
            continue
        sid = entry.get("source_id")
        eid = entry.get("id")
        if not eid or eid != sid:
            errors.append(f"{eid or '<missing-id>'}: id/source_id mismatch")
        if sid not in source_by_id:
            errors.append(f"{eid or '<missing-id>'}: unknown configured source {sid}")
            continue
        if entry.get("layer") != source_by_id[sid].get("layer"):
            errors.append(f"{sid}: taxonomy layer does not match source layer")
        if entry.get("category") not in allowed_categories:
            errors.append(f"{sid}: unsupported search category {entry.get('category')}")
        if entry.get("accessibility_validation") != "required-runtime":
            errors.append(f"{sid}: accessibility runtime validation is not required")

    mapped = set(ids)
    search_kinds = {
        "search-engine-reference",
        "privacy-search-reference",
        "privacy-search-onion-reference",
        "independent-search-reference",
        "metasearch-reference",
        "dark-web-search-reference",
        "scholarly-search-reference",
        "scholarly-deep-search-reference",
    }
    configured_search_ids = {
        s["id"] for s in sources
        if s.get("kind") in search_kinds and s.get("id")
    }
    missing = sorted(configured_search_ids - mapped)
    if missing:
        errors.append("configured search sources missing from taxonomy: " + ", ".join(missing))

    layers = {e.get("layer") for e in engines if isinstance(e, dict)}
    for layer in sorted(required_layers - layers):
        errors.append(f"search taxonomy missing publication layer: {layer}")

if not taxonomy.get("accessibility_invariant"):
    errors.append("search taxonomy missing accessibility invariant")
if not taxonomy.get("invariant"):
    errors.append("search taxonomy missing completeness invariant")

if errors:
    for error in errors:
        print("ERROR:", error)
    print(f"FAIL: {len(errors)} search-engine coverage error(s)")
    sys.exit(1)

print(f"PASS: {len(engines)} search engines/search services mapped")
print("PASS: surface, deep-unindexed and Tor/onion search layers represented")
print("PASS: every mapped search service requires runtime accessibility validation")
