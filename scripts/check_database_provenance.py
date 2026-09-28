#!/usr/bin/env python3
"""Runtime provenance contract checks for OMNI Security."""
from __future__ import annotations
import os
import psycopg

def main() -> int:
    dsn=os.environ.get("OMNI_DATABASE_URL")
    if not dsn: raise SystemExit("OMNI_DATABASE_URL is required")
    errors=[]
    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute("""SELECT COUNT(*) FROM evidence.source_observations o
                       JOIN evidence.evidence e ON e.id=o.evidence_id
                       WHERE o.observed_at IS NULL OR e.observed_at IS NULL""")
        if cur.fetchone()[0]: errors.append("source observations without observation timestamps")
        cur.execute("""SELECT COUNT(*) FROM intel.objects i
                       JOIN core.entities e ON e.id=i.entity_id
                       WHERE i.external_source IS NULL OR i.external_id IS NULL""")
        if cur.fetchone()[0]: errors.append("intel objects without external provenance")
        cur.execute("""SELECT COUNT(*) FROM evidence.claims c
                       WHERE NOT EXISTS (SELECT 1 FROM evidence.claim_sources cs WHERE cs.claim_id=c.id)""")
        if cur.fetchone()[0]: errors.append("claims without evidence provenance")
        cur.execute("""SELECT COUNT(*) FROM evidence.evidence e
                       WHERE NOT EXISTS (SELECT 1 FROM evidence.entity_evidence ee WHERE ee.evidence_id=e.id)""")
        if cur.fetchone()[0]: errors.append("evidence without entity provenance")
        cur.execute("""SELECT COUNT(*) FROM evidence.observation_artifacts oa
                       LEFT JOIN evidence.source_observations o ON o.id=oa.observation_id
                       LEFT JOIN evidence.artifacts a ON a.id=oa.artifact_id
                       WHERE o.id IS NULL OR a.id IS NULL""")
        if cur.fetchone()[0]: errors.append("broken observation/artifact provenance links")
        cur.execute("""SELECT COUNT(*) FROM evidence.artifact_claims ac
                       LEFT JOIN evidence.artifacts a ON a.id=ac.artifact_id
                       LEFT JOIN evidence.claims c ON c.id=ac.claim_id
                       WHERE a.id IS NULL OR c.id IS NULL""")
        if cur.fetchone()[0]: errors.append("broken artifact/claim provenance links")
        cur.execute("""SELECT COUNT(*) FROM evidence.claim_entities ce
                       LEFT JOIN evidence.claims c ON c.id=ce.claim_id
                       LEFT JOIN core.entities e ON e.id=ce.entity_id
                       WHERE c.id IS NULL OR e.id IS NULL""")
        if cur.fetchone()[0]: errors.append("broken claim/entity provenance links")
        cur.execute("""SELECT COUNT(*) FROM evidence.claims c
                       WHERE NOT EXISTS (
                         SELECT 1 FROM evidence.claim_entities ce WHERE ce.claim_id=c.id
                       )""")
        if cur.fetchone()[0]: errors.append("claims without entity provenance")
        cur.execute("""SELECT COUNT(*) FROM core.entity_relationships
                       WHERE source_evidence_id IS NULL""")
        unproven=cur.fetchone()[0]
        if unproven: print(f"INFO {unproven} relationships have no evidence reference")
    if errors:
        for e in errors: print(f"FAIL {e}")
        return 1
    print("PASS provenance contract")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
