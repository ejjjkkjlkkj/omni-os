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
    sources_doc = load_json(ROOT / "data/threat-intel/sources.json")
    sources = sources_doc.get("sources", []) if isinstance(sources_doc, dict) else sources_doc
    requirements = ROOT / "requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv"

    stack_layers = a11y.get("stack_layers", {})
    publication_layers = a11y.get("publication_layers", {})
    domains = master.get("domains", [])
    search_engines = list(search.get("engines", {}).values()) if isinstance(search.get("engines"), dict) else search.get("engines", [])
    search_code_sources = list(search.get("code_sources", {}).values()) if isinstance(search.get("code_sources"), dict) else search.get("code_sources", [])

    with conn.cursor() as cur:
        base_statuses = (
            ("configured", "Source is configured but not yet validated."),
            ("observed", "Entity or observation exists in imported evidence."),
            ("unverified", "Evidence has not been independently validated."),
            ("validated", "Evidence has passed the applicable validation."),
            ("failed", "The latest validation or ingestion failed."),
            ("blocked", "Collection or validation is blocked."),
            ("stale", "Evidence exists but is outside the freshness policy."),
        )
        for ident, description in base_statuses:
            cur.execute(
                "INSERT INTO taxonomy.statuses(id,description) VALUES (%s,%s) "
                "ON CONFLICT(id) DO UPDATE SET description=EXCLUDED.description",
                (ident, description),
            )
        for ident, description in (
            ("intel-object", "Normalized threat-intelligence object."),
            ("source", "Source or research reference."),
            ("platform", "Hardware or software platform."),
            ("firmware", "Firmware artifact or family."),
            ("boot-artifact", "Boot or pre-OS artifact."),
            ("accessibility", "Accessibility capability or implementation."),
        ):
            cur.execute(
                "INSERT INTO taxonomy.entity_types(id,description) VALUES (%s,%s) "
                "ON CONFLICT(id) DO UPDATE SET description=EXCLUDED.description",
                (ident, description),
            )
        for ident, description in (
            ("supports", "Source/entity supports another entity."),
            ("references", "One entity references another."),
            ("affects", "One entity affects another."),
            ("depends-on", "One entity depends on another."),
            ("derived-from", "Entity was derived from another observed object."),
        ):
            cur.execute(
                "INSERT INTO taxonomy.relationship_types(id,description) VALUES (%s,%s) "
                "ON CONFLICT(id) DO UPDATE SET description=EXCLUDED.description",
                (ident, description),
            )

        for key, row in stack_layers.items():
            number = int(key.removeprefix("L"))
            name = row.get("name", key)
            cur.execute(
                """
                INSERT INTO taxonomy.stack_layers(id, layer_number, name, description)
                VALUES (%s,%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET
                  layer_number=EXCLUDED.layer_number,
                  name=EXCLUDED.name,
                  description=EXCLUDED.description
                """,
                (key, number, name, row.get("description")),
            )

        for ident, row in publication_layers.items():
            label = row.get("name", ident) if isinstance(row, dict) else ident
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
                INSERT INTO taxonomy.domains(id, group_name, description, metadata)
                VALUES (%s,%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET
                  group_name=EXCLUDED.group_name,
                  description=EXCLUDED.description,
                  metadata=EXCLUDED.metadata
                """,
                (ident, row.get("group", "unknown"), row.get("description"), json.dumps(row)),
            )

        capability_ids = set()
        for row in a11y.get("capabilities", []):
            capability_ids.add(row["id"])
            cur.execute(
                """
                INSERT INTO accessibility.capabilities(id,name,category,description)
                VALUES (%s,%s,%s,%s)
                ON CONFLICT(id) DO UPDATE SET
                  name=EXCLUDED.name, category=EXCLUDED.category,
                  description=EXCLUDED.description
                """,
                (row["id"], row.get("name", row["id"]),
                 row.get("category", "accessibility"), row.get("description")),
            )

        for layer_id, row in stack_layers.items():
            for capability_id in row.get("accessibility", []):
                if capability_id not in capability_ids:
                    cur.execute(
                        """
                        INSERT INTO accessibility.capabilities(id,name,category)
                        VALUES (%s,%s,'accessibility')
                        ON CONFLICT(id) DO NOTHING
                        """,
                        (capability_id, capability_id),
                    )
                    capability_ids.add(capability_id)

        for row in search_engines:
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

        for row in search_engines:
            layer = row.get("layer")
            if layer:
                cur.execute(
                    """
                    INSERT INTO search.engine_layers(engine_id, publication_layer_id)
                    VALUES (%s,%s)
                    ON CONFLICT DO NOTHING
                    """,
                    (row["id"], layer),
                )

        for row in search_code_sources:
            cur.execute(
                """
                INSERT INTO search.code_sources
                  (id,repository,license,role,retrieval_policy,
                   security_review_status,accessibility_review_status,metadata)
                VALUES (%s,%s,%s,%s,%s,
                        'unverified','unverified',%s)
                ON CONFLICT(id) DO UPDATE SET
                  repository=EXCLUDED.repository, license=EXCLUDED.license,
                  role=EXCLUDED.role, metadata=EXCLUDED.metadata
                """,
                (row["id"], row["repository"], row.get("license"),
                 row.get("role"), row.get("retrieval", "reference-and-adapt-only"), json.dumps(row)),
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
            entity_type = record.get("entity_type", "intel-object")
            cur.execute(
                """
                INSERT INTO taxonomy.entity_types(id,description)
                VALUES (%s,%s)
                ON CONFLICT(id) DO NOTHING
                """,
                (entity_type, f"Imported canonical entity type: {entity_type}."),
            )
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
                    canonical, entity_type,
                    record.get("name") or external_id, json.dumps(record),
                ),
            )
            entity_id = cur.fetchone()[0]

            # Materialize the imported record as evidence and provenance.
            cur.execute("SELECT id FROM evidence.evidence WHERE content_hash=%s", (digest,))
            evidence_row = cur.fetchone()
            if evidence_row:
                evidence_id = evidence_row[0]
            else:
                cur.execute(
                    """
                    INSERT INTO evidence.evidence
                      (evidence_type,validation_status,observed_at,collector,method,
                       content_hash,content_ref,metadata)
                    VALUES ('intel-record','unverified',now(),'db_import',
                            'canonical-fixture-import',%s,%s,%s)
                    RETURNING id
                    """,
                    (digest, canonical, json.dumps({"source": source_id, "external_id": external_id})),
                )
                evidence_id = cur.fetchone()[0]

            cur.execute(
                "SELECT id FROM evidence.source_observations WHERE evidence_id=%s",
                (evidence_id,),
            )
            observation_row = cur.fetchone()
            if observation_row is None:
                cur.execute(
                    """
                    INSERT INTO evidence.source_observations
                      (source_id,evidence_id,external_id,observed_at,source_version,metadata)
                    VALUES (%s,%s,%s,now(),%s,%s)
                    RETURNING id
                    """,
                    (
                        source_row[0], evidence_id, external_id,
                        record.get("version") or record.get("modified") or "import",
                        json.dumps({"canonical_entity": canonical}),
                    ),
                )
                observation_id = cur.fetchone()[0]
            else:
                observation_id = observation_row[0]
            cur.execute(
                "SELECT id FROM evidence.artifacts WHERE evidence_id=%s AND content_hash=%s LIMIT 1",
                (evidence_id, digest),
            )
            artifact_row = cur.fetchone()
            if artifact_row is None:
                cur.execute(
                    """
                    INSERT INTO evidence.artifacts
                      (evidence_id,artifact_type,name,media_type,content_hash,size_bytes,metadata)
                    VALUES (%s,'canonical-record',%s,'application/json',%s,%s,%s)
                    RETURNING id
                    """,
                    (
                        evidence_id, canonical, digest,
                        len(raw.encode("utf-8")),
                        json.dumps({"source": source_id}),
                    ),
                )
                artifact_id = cur.fetchone()[0]
            else:
                artifact_id = artifact_row[0]

            cur.execute(
                """
                INSERT INTO evidence.observation_artifacts(observation_id,artifact_id)
                VALUES (%s,%s)
                ON CONFLICT DO NOTHING
                """,
                (observation_id, artifact_id),
            )
            cur.execute(
                """
                INSERT INTO evidence.entity_evidence(entity_id,evidence_id,role)
                VALUES (%s,%s,'source-record')
                ON CONFLICT DO NOTHING
                """,
                (entity_id, evidence_id),
            )
            statement = record.get("description") or record.get("name") or external_id
            cur.execute(
                "SELECT id FROM evidence.claims WHERE statement=%s LIMIT 1",
                (statement,),
            )
            claim_row = cur.fetchone()
            if claim_row:
                claim_id = claim_row[0]
            else:
                cur.execute(
                    """
                    INSERT INTO evidence.claims
                      (claim_type,statement,status,first_observed_at,metadata)
                    VALUES (%s,%s,'unverified',now(),%s)
                    RETURNING id
                    """,
                    (
                        entity_type, statement,
                        json.dumps({"entity_id": str(entity_id), "artifact_sha256": digest}),
                    ),
                )
                claim_id = cur.fetchone()[0]
            cur.execute(
                """
                INSERT INTO evidence.claim_sources(claim_id,evidence_id)
                VALUES (%s,%s)
                ON CONFLICT DO NOTHING
                """,
                (claim_id, evidence_id),
            )
            cur.execute(
                """
                INSERT INTO evidence.artifact_claims(artifact_id,claim_id)
                VALUES (%s,%s)
                ON CONFLICT DO NOTHING
                """,
                (artifact_id, claim_id),
            )
            cur.execute(
                """
                INSERT INTO evidence.claim_entities(claim_id,entity_id,role)
                VALUES (%s,%s,'subject')
                ON CONFLICT DO NOTHING
                """,
                (claim_id, entity_id),
            )

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
                (entity_id, external_id, source_id, entity_type,
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
