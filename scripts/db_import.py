#!/usr/bin/env python3
"""Idempotent import of OMNI canonical taxonomy/source data into PostgreSQL.

This importer never treats references as proof and never stores secret material.
Generated threat-intelligence records are imported only with their configured source
and evidence provenance.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
from typing import Any

import psycopg

ROOT = Path(__file__).resolve().parents[1]


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def secret_like(value: str) -> bool:
    lowered = value.lower()
    return (
        "begin rsa private key" in lowered
        or "begin openssh private key" in lowered
        or "begin ec private key" in lowered
        or "api_key=" in lowered
        or "password=" in lowered
        or "bearer " in lowered
    )


def upsert_reference_data(conn: psycopg.Connection) -> None:
    master = load_json(ROOT / "data/taxonomy/master-coverage.json")
    a11y = load_json(ROOT / "data/taxonomy/accessibility-full-stack.json")
    search = load_json(ROOT / "data/taxonomy/search-engine-coverage.json")
    sources = load_json(ROOT / "data/threat-intel/sources.json")
    requirements = ROOT / "requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv"

    stack_layers = a11y.get("stack_layers", [])
    publication_layers = a11y.get("publication_layers", [])
    domains = master.get("domains", [])

    with conn.cursor() as cur:
        for row in stack_layers:
            number = row.get("number", row.get("layer"))
            name = row.get("id", row.get("name"))
            if number is None or name is None:
                continue
            cur.execute(
                """
                INSERT INTO taxonomy.stack_layers(id, layer_number, name, description)
                VALUES (%s,%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET
                  layer_number=EXCLUDED.layer_number,
                  name=EXCLUDED.name,
                  description=EXCLUDED.description
                """,
                (name, int(number), row.get("name", name), row.get("description")),
            )

        for row in publication_layers:
            ident = row if isinstance(row, str) else row.get("id")
            if not ident:
                continue
            label = ident if isinstance(row, str) else row.get("name", ident)
            cur.execute(
                """
                INSERT INTO taxonomy.publication_layers(id, name)
                VALUES (%s,%s)
                ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name
                """,
                (ident, label),
            )

        for row in domains:
            ident = row["id"]
            cur.execute(
                """
                INSERT INTO taxonomy.domains(id, group_name, metadata)
                VALUES (%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET
                  group_name=EXCLUDED.group_name,
                  metadata=EXCLUDED.metadata
                """,
                (ident, row.get("group", "unknown"), json.dumps(row)),
            )

        for row in sources:
            if secret_like(json.dumps(row, sort_keys=True)):
                raise RuntimeError(f"secret-like source record rejected: {row.get('id')}")
            layer = row.get("layer")
            cur.execute(
                """
                INSERT INTO source.sources
                  (canonical_id,name,kind,domain,url,publication_layer_id,required,mode,
                   status,license,metadata)
                VALUES (%s,%s,%s,%s,%s,%s,%s,%s,'configured',%s,%s)
                ON CONFLICT(canonical_id) DO UPDATE SET
                  name=EXCLUDED.name, kind=EXCLUDED.kind, domain=EXCLUDED.domain,
                  url=EXCLUDED.url, publication_layer_id=EXCLUDED.publication_layer_id,
                  required=EXCLUDED.required, mode=EXCLUDED.mode,
                  license=EXCLUDED.license, metadata=EXCLUDED.metadata
                """,
                (
                    row["id"], row.get("name", row["id"]), row.get("kind", "unknown"),
                    row.get("domain"), row.get("url"), layer, bool(row.get("required", False)),
                    row.get("mode", "reference"), row.get("license"),
                    json.dumps(row),
                ),
            )

        for row in search.get("engines", []):
            cur.execute(
                """
                INSERT INTO search.engines
                  (id,name,category,code_reference_status,repository_url,publication_layer_id,metadata)
                VALUES (%s,%s,%s,%s,%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET
                  name=EXCLUDED.name, category=EXCLUDED.category,
                  code_reference_status=EXCLUDED.code_reference_status,
                  repository_url=EXCLUDED.repository_url,
                  publication_layer_id=EXCLUDED.publication_layer_id,
                  metadata=EXCLUDED.metadata
                """,
                (
                    row["id"], row.get("name", row["id"]), row.get("category", "unknown"),
                    row.get("code_reference_status", "to-verify"), row.get("code_reference"),
                    row.get("layer"), json.dumps(row),
                ),
            )

        with requirements.open(newline="", encoding="utf-8") as fh:
            for row in csv.DictReader(fh):
                cur.execute(
                    """
                    INSERT INTO accessibility.requirements
                      (id,name,description,security_property,accessibility_property,
                       required_evidence,status)
                    VALUES (%s,%s,%s,%s,%s,%s,%s)
                    ON CONFLICT(id) DO UPDATE SET
                      name=EXCLUDED.name, description=EXCLUDED.description,
                      security_property=EXCLUDED.security_property,
                      accessibility_property=EXCLUDED.accessibility_property,
                      required_evidence=EXCLUDED.required_evidence,
                      status=EXCLUDED.status
                    """,
                    (
                        row["id"], row["domain"], row["requirement"],
                        row["security_property"], row["accessibility_property"],
                        row["required_evidence"], row["status"],
                    ),
                )

        # Canonical screen-reader set required by the architecture.
        for ident, name, platform in (
            ("nvda", "NVDA", "Windows"),
            ("jaws", "JAWS", "Windows"),
            ("narrator", "Narrator", "Windows"),
            ("voiceover", "VoiceOver", "Apple"),
            ("talkback", "TalkBack", "Android"),
            ("orca", "Orca", "Linux"),
            ("brltty", "BRLTTY", "Linux"),
        ):
            cur.execute(
                """
                INSERT INTO accessibility.screen_readers(id,name,platform)
                VALUES (%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name, platform=EXCLUDED.platform
                """,
                (ident, name, platform),
            )

    conn.commit()


def import_knowledge(conn: psycopg.Connection, limit: int | None = None) -> int:
    path = ROOT / "data/threat-intel/generated/security-knowledge.json"
    data = load_json(path)
    records = data.get("records", [])
    if limit is not None:
        records = records[:limit]

    count = 0
    with conn.cursor() as cur:
        for record in records:
            raw = json.dumps(record, sort_keys=True, ensure_ascii=False)
            if secret_like(raw):
                raise RuntimeError("secret-like content detected in threat-intel record")
            source_id = record.get("source")
            external_id = record.get("id") or record.get("external_id")
            if not source_id or not external_id:
                continue
            canonical = f"intel:{source_id}:{external_id}"
            digest = hashlib.sha256(raw.encode("utf-8")).hexdigest()
            cur.execute("SELECT id FROM source.sources WHERE canonical_id=%s", (source_id,))
            source_row = cur.fetchone()
            if source_row is None:
                raise RuntimeError(f"unknown configured source: {source_id}")
            cur.execute(
                """
                INSERT INTO core.entities(canonical_id,entity_type,name,status,metadata)
                VALUES (%s,%s,%s,'observed',%s)
                ON CONFLICT(canonical_id) DO UPDATE SET
                  entity_type=EXCLUDED.entity_type,
                  name=EXCLUDED.name,
                  updated_at=now(),
                  metadata=EXCLUDED.metadata
                RETURNING id
                """,
                (
                    canonical, record.get("entity_type", "intel-object"),
                    record.get("name") or external_id, json.dumps(record),
                ),
            )
            entity_id = cur.fetchone()[0]
            cur.execute(
                """
                INSERT INTO intel.objects(entity_id,external_id,external_source,object_type,raw,normalized)
                VALUES (%s,%s,%s,%s,%s,%s)
                ON CONFLICT(entity_id) DO UPDATE SET
                  external_id=EXCLUDED.external_id,
                  external_source=EXCLUDED.external_source,
                  object_type=EXCLUDED.object_type,
                  raw=EXCLUDED.raw,
                  normalized=EXCLUDED.normalized
                """,
                (entity_id, external_id, source_id, record.get("entity_type", "intel-object"),
                 json.dumps(record), json.dumps({"sha256": digest})),
            )
            count += 1
        conn.commit()
    return count


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dsn", default=os.environ.get("OMNI_DATABASE_URL"))
    parser.add_argument("--knowledge-limit", type=int)
    args = parser.parse_args()
    if not args.dsn:
        parser.error("OMNI_DATABASE_URL or --dsn is required")
    with psycopg.connect(args.dsn) as conn:
        upsert_reference_data(conn)
        imported = import_knowledge(conn, args.knowledge_limit)
    print(f"PASS database import: {imported} knowledge records processed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
