#!/usr/bin/env python3
"""Refresh OMNI public defensive security knowledge.

Only public HTTP(S) feeds marked as machine-readable are fetched here.
Deep/dark/overlay observations are imported separately as normalized metadata.
Raw personal data and secrets are outside the repository data model.
"""
from __future__ import annotations
import datetime as dt
import hashlib
import json
import pathlib
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
CFG = json.loads((ROOT / "data/threat-intel/sources.json").read_text(encoding="utf-8"))
OUT = ROOT / "data/threat-intel/generated"
OUT.mkdir(parents=True, exist_ok=True)
UA = "OMNI-Security-Knowledge-Updater/1.0"

FETCH_KINDS = {"attack-stix", "misp-cluster", "cisa-kev"}

def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=90) as r:
        return r.read()

def slim_attack(source_id: str, obj: dict) -> list[dict]:
    keep = {"intrusion-set", "campaign", "malware", "tool", "attack-pattern"}
    rows = []
    for item in obj.get("objects", []):
        typ = item.get("type")
        if typ not in keep:
            continue
        refs = item.get("external_references") or []
        rows.append({
            "source": source_id,
            "entity_type": typ,
            "id": item.get("id"),
            "name": item.get("name"),
            "aliases": item.get("aliases") or [],
            "external_ids": [r.get("external_id") for r in refs if r.get("external_id")],
            "source_urls": [r.get("url") for r in refs if r.get("url")],
            "created": item.get("created"),
            "modified": item.get("modified"),
            "revoked": bool(item.get("revoked", False)),
        })
    return rows

def slim_misp(source_id: str, obj: dict) -> list[dict]:
    rows = []
    for item in obj.get("values", []):
        meta = item.get("meta") or {}
        rows.append({
            "source": source_id,
            "entity_type": obj.get("type") or "misp-cluster",
            "id": item.get("uuid"),
            "name": item.get("value"),
            "aliases": meta.get("synonyms") or [],
            "refs": meta.get("refs") or [],
            "description": (item.get("description") or "")[:2000],
        })
    return rows

def slim_kev(source_id: str, obj: dict) -> list[dict]:
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
    "schema_version": 1,
    "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
    "policy": CFG.get("personal_data_policy"),
    "sources": [],
}

for source in CFG["sources"]:
    sid = source["id"]
    kind = source["kind"]
    layer = source["layer"]
    url = source.get("url")
    mode = source.get("mode")

    if mode == "import-only" or kind not in FETCH_KINDS:
        manifest["sources"].append({
            "id": sid, "layer": layer, "kind": kind,
            "mode": mode or "reference", "url": url,
        })
        continue

    try:
        raw = fetch(url)
        digest = hashlib.sha256(raw).hexdigest()
        obj = json.loads(raw)
        if kind == "attack-stix":
            rows = slim_attack(sid, obj)
        elif kind == "misp-cluster":
            rows = slim_misp(sid, obj)
        elif kind == "cisa-kev":
            rows = slim_kev(sid, obj)
        else:
            rows = []
        records.extend(rows)
        manifest["sources"].append({
            "id": sid, "layer": layer, "kind": kind, "url": url,
            "sha256": digest, "records": len(rows),
        })
    except Exception as exc:
        manifest["sources"].append({
            "id": sid, "layer": layer, "kind": kind, "url": url,
            "error": f"{type(exc).__name__}: {exc}",
        })
        if source.get("required"):
            raise

records.sort(key=lambda r: (
    r.get("entity_type") or "",
    r.get("name") or "",
    r.get("id") or "",
))

(OUT / "security-knowledge.json").write_text(
    json.dumps({"schema_version": 1, "records": records}, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
(OUT / "manifest.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
print(f"OMNI knowledge refresh: {len(records)} normalized records")
