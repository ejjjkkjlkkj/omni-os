#!/usr/bin/env python3
"""Build OMNI's local public security-intelligence cache.

Only machine-readable public feeds are fetched automatically.
Deep/dark/overlay observations are imported separately as normalized metadata.
The repository stores security metadata, not raw private-person data or secrets.
"""
from __future__ import annotations
import hashlib
import json
import pathlib
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
CFG = json.loads((ROOT / "data/threat-intel/sources.json").read_text(encoding="utf-8"))
OUT = ROOT / "data/threat-intel/generated"
OUT.mkdir(parents=True, exist_ok=True)
UA = "OMNI-Security-Knowledge-Updater/2.0"
FETCH_KINDS = {"attack-stix", "misp-cluster", "cisa-kev"}

def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "application/json,*/*"})
    with urllib.request.urlopen(req, timeout=90) as response:
        return response.read()

def attack_records(source_id: str, obj: dict) -> list[dict]:
    rows: list[dict] = []
    entity_types = {"intrusion-set", "campaign", "malware", "tool", "attack-pattern"}
    for item in obj.get("objects", []):
        typ = item.get("type")
        if typ in entity_types:
            refs = item.get("external_references") or []
            rows.append({
                "source": source_id,
                "entity_type": typ,
                "id": item.get("id"),
                "name": item.get("name"),
                "aliases": sorted(set(item.get("aliases") or [])),
                "external_ids": sorted({r.get("external_id") for r in refs if r.get("external_id")}),
                "references": sorted({r.get("url") for r in refs if r.get("url")}),
                "created": item.get("created"),
                "modified": item.get("modified"),
                "revoked": bool(item.get("revoked", False)),
            })
        elif typ == "relationship":
            rows.append({
                "source": source_id,
                "entity_type": "relationship",
                "id": item.get("id"),
                "relationship_type": item.get("relationship_type"),
                "source_ref": item.get("source_ref"),
                "target_ref": item.get("target_ref"),
                "created": item.get("created"),
                "modified": item.get("modified"),
            })
    return rows

def misp_records(source_id: str, obj: dict) -> list[dict]:
    rows: list[dict] = []
    entity_type = obj.get("type") or "misp-cluster"
    for item in obj.get("values", []):
        meta = item.get("meta") or {}
        # Do not mirror free-form descriptions: they may contain unnecessary
        # victim/private-person information. Keep technical metadata only.
        rows.append({
            "source": source_id,
            "entity_type": entity_type,
            "id": item.get("uuid"),
            "name": item.get("value"),
            "aliases": sorted({x for x in (meta.get("synonyms") or []) if isinstance(x, str)}),
            "references": sorted({x for x in (meta.get("refs") or []) if isinstance(x, str)}),
        })
    return rows

def kev_records(source_id: str, obj: dict) -> list[dict]:
    return [{
        "source": source_id,
        "entity_type": "known-exploited-vulnerability",
        "id": v.get("cveID"),
        "name": v.get("vulnerabilityName"),
        "vendor": v.get("vendorProject"),
        "product": v.get("product"),
        "date_added": v.get("dateAdded"),
        "due_date": v.get("dueDate"),
        "known_ransomware_use": v.get("knownRansomwareCampaignUse"),
    } for v in obj.get("vulnerabilities", [])]

records: list[dict] = []
manifest = {
    "schema_version": 2,
    "policy": CFG.get("personal_data_policy"),
    "sources": [],
}

for source in CFG["sources"]:
    sid = source["id"]
    kind = source["kind"]
    layer = source.get("layer")
    url = source.get("url")
    mode = source.get("mode")
    entry = {"id": sid, "layer": layer, "kind": kind, "url": url, "mode": mode or "automatic"}

    if mode == "import-only" or kind not in FETCH_KINDS:
        manifest["sources"].append(entry)
        continue

    try:
        raw = fetch(url)
        entry["sha256"] = hashlib.sha256(raw).hexdigest()
        obj = json.loads(raw)
        if kind == "attack-stix":
            rows = attack_records(sid, obj)
        elif kind == "misp-cluster":
            rows = misp_records(sid, obj)
        elif kind == "cisa-kev":
            rows = kev_records(sid, obj)
        else:
            rows = []
        records.extend(rows)
        entry["records"] = len(rows)
    except Exception as exc:
        entry["error"] = f"{type(exc).__name__}: {exc}"
        manifest["sources"].append(entry)
        if source.get("required"):
            raise
        continue
    manifest["sources"].append(entry)

records.sort(key=lambda r: (
    r.get("entity_type") or "",
    r.get("name") or "",
    r.get("id") or "",
    r.get("source") or "",
))

knowledge = {
    "schema_version": 2,
    "personal_data_policy": "security metadata only; no raw credentials, secrets, private communications or private-person dossiers",
    "record_count": len(records),
    "records": records,
}

(OUT / "security-knowledge.json").write_text(
    json.dumps(knowledge, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
(OUT / "manifest.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
print(f"PASS: {len(records)} normalized records from {len(CFG['sources'])} configured sources")
