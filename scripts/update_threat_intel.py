#!/usr/bin/env python3
"""Build OMNI's local public security-intelligence cache.

Automatically fetched sources are public machine-readable feeds.
Deep/dark/overlay observations are imported separately as normalized metadata.
The repository stores security metadata, not raw private-person data or secrets.
"""
from __future__ import annotations
import collections
import hashlib
import io
import json
import pathlib
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
CFG = json.loads((ROOT / "data/threat-intel/sources.json").read_text(encoding="utf-8"))
OUT = ROOT / "data/threat-intel/generated"
OUT.mkdir(parents=True, exist_ok=True)
UA = "OMNI-Security-Knowledge-Updater/4.0"
FETCH_KINDS = {
    "attack-stix", "misp-cluster", "cisa-kev",
    "d3fend-jsonld", "capec-xml", "cwe-zip-xml"
}

def fetch(url: str) -> bytes:
    if not isinstance(url, str) or not url.startswith("https://"):
        raise ValueError(f"source URL must use HTTPS: {url!r}")
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=120) as response:
        content_length = response.headers.get("Content-Length")
        if content_length and int(content_length) > 64 * 1024 * 1024:
            raise ValueError("source response exceeds 64 MiB safety limit")
        raw = response.read(64 * 1024 * 1024 + 1)
        if len(raw) > 64 * 1024 * 1024:
            raise ValueError("source response exceeds 64 MiB safety limit")
        return raw

def atomic_write(path: pathlib.Path, payload: str) -> None:
    tmp = path.with_name(path.name + ".tmp")
    try:
        tmp.write_text(payload, encoding="utf-8")
        tmp.replace(path)
    finally:
        if tmp.exists():
            tmp.unlink()

def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]

def child_text(node: ET.Element, wanted: str) -> str | None:
    for child in node.iter():
        if local_name(child.tag) == wanted and child.text:
            return " ".join(child.text.split())[:2000]
    return None

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

def jsonld_value(value):
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        return value.get("@value") or value.get("@id")
    if isinstance(value, list):
        for x in value:
            v = jsonld_value(x)
            if v:
                return v
    return None

def d3fend_records(source_id: str, obj) -> list[dict]:
    graph = obj.get("@graph", []) if isinstance(obj, dict) else obj if isinstance(obj, list) else []
    rows = []
    for item in graph:
        if not isinstance(item, dict) or not item.get("@id"):
            continue
        label = None
        for key, value in item.items():
            tail = key.rsplit("#", 1)[-1].rsplit("/", 1)[-1].lower()
            if tail in {"label", "preflabel"}:
                label = jsonld_value(value)
                if label:
                    break
        types = item.get("@type") or []
        if isinstance(types, str):
            types = [types]
        rows.append({
            "source": source_id,
            "entity_type": "defensive-knowledge",
            "id": item.get("@id"),
            "name": label,
            "classes": sorted(str(x) for x in types),
        })
    return rows

def capec_records(source_id: str, raw: bytes) -> list[dict]:
    root = ET.fromstring(raw)
    rows = []
    for item in root.iter():
        if local_name(item.tag) != "Attack_Pattern":
            continue
        capec_id = item.attrib.get("ID")
        rows.append({
            "source": source_id,
            "entity_type": "attack-pattern",
            "id": f"CAPEC-{capec_id}" if capec_id else None,
            "name": item.attrib.get("Name"),
            "abstraction": item.attrib.get("Abstraction"),
            "status": item.attrib.get("Status"),
            "likelihood": child_text(item, "Likelihood_Of_Attack"),
            "severity": child_text(item, "Typical_Severity"),
        })
    return rows

def cwe_records(source_id: str, raw: bytes) -> list[dict]:
    with zipfile.ZipFile(io.BytesIO(raw)) as zf:
        xml_names = [n for n in zf.namelist() if n.lower().endswith(".xml")]
        if not xml_names:
            raise ValueError("CWE ZIP contains no XML file")
        xml = zf.read(sorted(xml_names)[0])
    root = ET.fromstring(xml)
    rows = []
    for item in root.iter():
        if local_name(item.tag) != "Weakness":
            continue
        cwe_id = item.attrib.get("ID")
        rows.append({
            "source": source_id,
            "entity_type": "weakness",
            "id": f"CWE-{cwe_id}" if cwe_id else None,
            "name": item.attrib.get("Name"),
            "abstraction": item.attrib.get("Abstraction"),
            "structure": item.attrib.get("Structure"),
            "status": item.attrib.get("Status"),
        })
    return rows

def decorate(rows: list[dict], source: dict) -> list[dict]:
    domain = source.get("domain") or "cybersecurity"
    layer = source.get("layer")
    kind = source.get("kind")
    for row in rows:
        row.setdefault("domain", domain)
        row.setdefault("layer", layer)
        row.setdefault("source_kind", kind)
    return rows

def source_reference(source: dict) -> dict:
    return {
        "source": source["id"],
        "domain": source.get("domain") or "cybersecurity",
        "layer": source.get("layer"),
        "source_kind": source.get("kind"),
        "entity_type": "source-reference",
        "id": source["id"],
        "name": source["id"],
        "references": [source["url"]] if source.get("url") else [],
        "mode": source.get("mode") or "reference",
    }

records: list[dict] = []
manifest = {
    "schema_version": 4,
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
        records.append(source_reference(source))
        manifest["sources"].append(entry)
        continue

    try:
        raw = fetch(url)
        entry["sha256"] = hashlib.sha256(raw).hexdigest()
        if kind == "attack-stix":
            rows = attack_records(sid, json.loads(raw))
        elif kind == "misp-cluster":
            rows = misp_records(sid, json.loads(raw))
        elif kind == "cisa-kev":
            rows = kev_records(sid, json.loads(raw))
        elif kind == "d3fend-jsonld":
            rows = d3fend_records(sid, json.loads(raw))
        elif kind == "capec-xml":
            rows = capec_records(sid, raw)
        elif kind == "cwe-zip-xml":
            rows = cwe_records(sid, raw)
        else:
            rows = []
        rows = decorate(rows, source)
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

source_counts = collections.Counter(r.get("source") or "unknown" for r in records)
type_counts = collections.Counter(r.get("entity_type") or "unknown" for r in records)
domain_counts = collections.Counter(r.get("domain") or "unknown" for r in records)
layer_counts = collections.Counter(r.get("layer") or "unknown" for r in records)
knowledge = {
    "schema_version": 4,
    "personal_data_policy": "security metadata only; no raw credentials, secrets, private communications or private-person dossiers",
    "record_count": len(records),
    "records": records,
}
catalog = {
    "schema_version": 1,
    "record_count": len(records),
    "source_counts": dict(sorted(source_counts.items())),
    "type_counts": dict(sorted(type_counts.items())),
    "domain_counts": dict(sorted(domain_counts.items())),
    "layer_counts": dict(sorted(layer_counts.items())),
}

atomic_write(
    OUT / "security-knowledge.json",
    json.dumps(knowledge, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
)
atomic_write(
    OUT / "catalog.json",
    json.dumps(catalog, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
)
atomic_write(
    OUT / "manifest.json",
    json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
)
print(f"PASS: {len(records)} normalized records from {len(CFG['sources'])} configured sources")
