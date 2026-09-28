#!/usr/bin/env python3
"""Fail-closed validation for OMNI knowledge coverage."""
from __future__ import annotations
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent
KNOWLEDGE = ROOT / "knowledge"


def load(name):
    return json.loads((KNOWLEDGE / name).read_text(encoding="utf-8"))


def validate():
    errors = []
    schema = load("schema.json")
    requirements = load("requirements.json")
    coverage = load("coverage.json")
    engineering = load("engineering-map.json")

    if schema.get("rule") != "UNKNOWN never becomes PASS automatically":
        errors.append("schema unknown rule missing")

    required_dimensions = {
        "correctness", "security", "accessibility", "compatibility",
        "performance", "maintainability", "tests", "evidence",
    }
    if not required_dimensions.issubset(set(schema.get("dimensions", []))):
        errors.append("required engineering dimensions missing")

    required_domains = set(coverage.get("domains", []))
    mapped_domains = set(engineering.get("coverage_domains", []))
    if not required_domains.issubset(mapped_domains):
        errors.append("coverage domain is not represented in engineering map")

    if len(engineering.get("non_negotiables", [])) < 10:
        errors.append("non-negotiable contract is incomplete")

    seen = set()
    for req in requirements.get("requirements", []):
        rid = req.get("id")
        if not rid or rid in seen:
            errors.append("duplicate or missing requirement id")
        seen.add(rid or "")
        if not req.get("source_refs"):
            errors.append("requirement without provenance source refs: " + str(rid))
        if req.get("status") == "PASS":
            errors.append("PASS is not a valid knowledge requirement state: " + str(rid))

    return errors


def main():
    errors = validate()
    print(json.dumps({"status": "PASS" if not errors else "FAIL", "errors": errors}, indent=2))
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
