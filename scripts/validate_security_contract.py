#!/usr/bin/env python3
"""Fail-closed validation for OMNI security/accessibility repository contracts."""
from __future__ import annotations

import csv
import json
import pathlib
import re
import sys
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parents[1]
ERRORS: list[str] = []

def load_json(rel: str):
    path = ROOT / rel
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        ERRORS.append(f"{rel}: invalid JSON: {type(exc).__name__}: {exc}")
        return {}

def require_file(rel: str) -> pathlib.Path | None:
    path = ROOT / rel
    if not path.is_file() or path.stat().st_size == 0:
        ERRORS.append(f"required file missing or empty: {rel}")
        return None
    return path

def check_requirements() -> None:
    path = require_file("requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv")
    if not path:
        return
    required = {
        "id", "domain", "requirement", "security_property",
        "accessibility_property", "required_evidence", "status",
    }
    seen: set[str] = set()
    implemented = 0
    try:
        with path.open(encoding="utf-8", newline="") as fh:
            rows = list(csv.DictReader(fh))
        if not rows:
            ERRORS.append("requirements CSV contains no rows")
            return
        missing = required - set(rows[0])
        if missing:
            ERRORS.append("requirements CSV missing columns: " + ", ".join(sorted(missing)))
        for row in rows:
            rid = (row.get("id") or "").strip()
            if not rid:
                ERRORS.append("requirements CSV contains a row without id")
                continue
            if rid in seen:
                ERRORS.append(f"duplicate requirement id: {rid}")
            seen.add(rid)
            for field in required:
                if not (row.get(field) or "").strip():
                    ERRORS.append(f"{rid}: empty {field}")
            if row.get("status") == "implemented":
                implemented += 1
        if len(seen) < 1:
            ERRORS.append("no security/accessibility requirements found")
        print(f"PASS: {len(seen)} security/accessibility requirements; {implemented} marked implemented")
    except Exception as exc:
        ERRORS.append(f"requirements CSV unreadable: {type(exc).__name__}: {exc}")

def check_schema() -> None:
    schema = load_json("data/schema/security-knowledge.schema.json")
    for key in ("$schema", "title", "type", "required", "properties"):
        if key not in schema:
            ERRORS.append(f"security knowledge schema missing {key}")
    if schema.get("type") != "object":
        ERRORS.append("security knowledge schema root must be object")
    props = schema.get("properties", {})
    for key in ("schema_version", "personal_data_policy", "record_count", "records"):
        if key not in props:
            ERRORS.append(f"security knowledge schema missing property {key}")

def check_sources() -> None:
    doc = load_json("data/threat-intel/sources.json")
    sources = doc.get("sources")
    if not isinstance(sources, list) or not sources:
        ERRORS.append("sources.json must contain a non-empty sources array")
        return
    ids = [s.get("id") for s in sources]
    dupes = [k for k, n in Counter(ids).items() if k and n > 1]
    for sid in dupes:
        ERRORS.append(f"duplicate threat-intelligence source id: {sid}")
    for source in sources:
        sid = source.get("id") or "<missing-id>"
        mode = source.get("mode", "reference")
        if not source.get("domain"):
            ERRORS.append(f"{sid}: missing domain")
        if not source.get("kind"):
            ERRORS.append(f"{sid}: missing kind")
        if mode != "import-only" and not source.get("url"):
            ERRORS.append(f"{sid}: non-import source has no URL")
        url = source.get("url")
        if url and not re.match(r"^https://", url):
            ERRORS.append(f"{sid}: source URL must use HTTPS")
    print(f"PASS: {len(sources)} threat-intelligence source definitions")

def check_generated() -> None:
    path = ROOT / "data/threat-intel/generated/security-knowledge.json"
    if not path.exists():
        print("INFO: generated security knowledge cache is not present; updater CI will create it")
        return
    data = load_json("data/threat-intel/generated/security-knowledge.json")
    records = data.get("records")
    if not isinstance(records, list):
        ERRORS.append("generated security knowledge records must be an array")
        return
    if data.get("record_count") != len(records):
        ERRORS.append(
            f"generated record_count={data.get('record_count')} but actual records={len(records)}"
        )
    for idx, record in enumerate(records):
        if not record.get("source") or not record.get("entity_type"):
            ERRORS.append(f"generated record {idx} lacks source/entity_type")
            break
    print(f"PASS: generated cache contains {len(records)} normalized records")

def check_workflows() -> None:
    workflow_dir = ROOT / ".github/workflows"
    sha_re = re.compile(r"^[0-9a-f]{40}$")
    if not workflow_dir.is_dir():
        ERRORS.append(".github/workflows is missing")
        return
    count = 0
    for path in sorted(workflow_dir.glob("*.y*ml")):
        count += 1
        text = path.read_text(encoding="utf-8")
        if re.search(r"^\\s*pull_request_target\\s*:", text, re.MULTILINE):
            ERRORS.append(f"{path}: pull_request_target is forbidden")
        if re.search(r"permissions:\s*write-all", text):
            ERRORS.append(f"{path}: write-all permissions are forbidden")
        for line_no, line in enumerate(text.splitlines(), 1):
            match = re.search(r"\buses:\s*([^\s#]+)", line)
            if not match:
                continue
            ref = match.group(1)
            if ref.startswith(("./", "docker://")):
                continue
            if "@" not in ref or not sha_re.fullmatch(ref.rsplit("@", 1)[1]):
                ERRORS.append(f"{path}:{line_no}: action is not pinned to a full commit SHA: {ref}")
    print(f"PASS: {count} GitHub workflow files checked")

def check_private_material() -> None:
    patterns = (
        b"-----BEGIN " + b"PRIVATE KEY-----",
        b"-----BEGIN " + b"RSA PRIVATE KEY-----",
        b"-----BEGIN " + b"EC PRIVATE KEY-----",
        b"-----BEGIN " + b"OPENSSH PRIVATE KEY-----",
    )
    for path in ROOT.rglob("*"):
        if not path.is_file():
            continue
        rel = path.relative_to(ROOT).as_posix()
        if rel.startswith(".git/") or rel.endswith(".md") or rel == "SECURITY.md" or "__pycache__/" in rel or rel.endswith(".pyc"):
            continue
        try:
            raw = path.read_bytes()
        except OSError:
            continue
        if any(marker in raw for marker in patterns):
            ERRORS.append(f"possible private-key material detected: {rel}")
    print("PASS: no committed private-key PEM blocks detected")

def check_generated_cross_artifacts() -> None:
    knowledge_path = ROOT / "data/threat-intel/generated/security-knowledge.json"
    catalog_path = ROOT / "data/threat-intel/generated/catalog.json"
    manifest_path = ROOT / "data/threat-intel/generated/manifest.json"
    if not all(p.exists() for p in (knowledge_path, catalog_path, manifest_path)):
        return

    knowledge = load_json("data/threat-intel/generated/security-knowledge.json")
    catalog = load_json("data/threat-intel/generated/catalog.json")
    manifest = load_json("data/threat-intel/generated/manifest.json")
    configured = load_json("data/threat-intel/sources.json").get("sources", [])
    configured_ids = {s.get("id") for s in configured if s.get("id")}

    manifest_sources = manifest.get("sources")
    if not isinstance(manifest_sources, list):
        ERRORS.append("generated manifest sources must be an array")
        return
    manifest_ids = [s.get("id") for s in manifest_sources]
    if set(manifest_ids) != configured_ids or len(manifest_ids) != len(configured_ids):
        ERRORS.append("generated manifest source IDs do not exactly match configured source IDs")

    records = knowledge.get("records", [])
    unknown = sorted({r.get("source") for r in records if r.get("source") not in configured_ids})
    if unknown:
        ERRORS.append("generated records reference unknown sources: " + ", ".join(str(x) for x in unknown))

    expected_counts = Counter(r.get("source") or "unknown" for r in records)
    if catalog.get("record_count") != len(records):
        ERRORS.append("generated catalog record_count does not match knowledge records")
    for key in ("source_counts", "type_counts", "domain_counts", "layer_counts"):
        value = catalog.get(key)
        if not isinstance(value, dict):
            ERRORS.append(f"generated catalog missing object: {key}")
    expected_type_counts = Counter(r.get("entity_type") or "unknown" for r in records)
    expected_domain_counts = Counter(r.get("domain") or "unknown" for r in records)
    expected_layer_counts = Counter(r.get("layer") or "unknown" for r in records)
    expected_catalog_counts = {
        "source_counts": dict(sorted(expected_counts.items())),
        "type_counts": dict(sorted(expected_type_counts.items())),
        "domain_counts": dict(sorted(expected_domain_counts.items())),
        "layer_counts": dict(sorted(expected_layer_counts.items())),
    }
    for key, expected in expected_catalog_counts.items():
        if isinstance(catalog.get(key), dict) and catalog[key] != expected:
            ERRORS.append(f"generated catalog {key} do not match knowledge records")

    configured_by_id = {s.get("id"): s for s in configured if s.get("id")}
    manifest_by_id = {}
    for entry in manifest_sources:
        if isinstance(entry, dict) and entry.get("id"):
            manifest_by_id[entry["id"]] = entry

    for sid, source in configured_by_id.items():
        entry = manifest_by_id.get(sid)
        if entry is None:
            continue
        declared = entry.get("records")
        actual = expected_counts.get(sid, 0)
        if entry.get("error"):
            if source.get("required") is True:
                ERRORS.append(f"required configured source failed: {sid}")
            elif actual != 0:
                ERRORS.append(f"optional failed source {sid} unexpectedly produced records={actual}")
        else:
            if not isinstance(declared, int):
                ERRORS.append(f"generated manifest missing records count for successful source: {sid}")
            elif declared != actual:
                ERRORS.append(f"generated manifest records for {sid}={declared} but actual records={actual}")

    for entry in manifest_sources:
        if not isinstance(entry, dict) or not entry.get("id"):
            ERRORS.append("generated manifest contains an invalid source entry")
            continue
        if entry.get("mode") != "import-only" and entry.get("kind") in {
            "attack-stix", "misp-cluster", "cisa-kev", "d3fend-jsonld", "capec-xml", "cwe-zip-xml"
        }:
            if "error" in entry and entry.get("required"):
                ERRORS.append(f"required generated source failed: {entry.get('id')}")

    print("PASS: generated knowledge, catalog, and manifest cross-checks complete")

def check_deterministic_hashes() -> None:
    manifest_path = ROOT / "data/threat-intel/generated/manifest.json"
    if not manifest_path.exists():
        return
    manifest = load_json("data/threat-intel/generated/manifest.json")
    for source in manifest.get("sources", []):
        digest = source.get("sha256")
        if digest is not None and not re.fullmatch(r"[0-9a-f]{64}", digest):
            ERRORS.append(f"{source.get('id','<unknown>')}: invalid SHA-256 digest in manifest")
    print("PASS: generated source digests use SHA-256 format")

check_requirements()
check_schema()
check_sources()
check_generated()
check_generated_cross_artifacts()
check_workflows()
check_private_material()
check_deterministic_hashes()

if ERRORS:
    for error in ERRORS:
        print("ERROR:", error)
    print(f"FAIL: {len(ERRORS)} security contract error(s)")
    sys.exit(1)

print("PASS: OMNI security contract validation complete")
