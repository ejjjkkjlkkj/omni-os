#!/usr/bin/env python3
"""Runtime PostgreSQL integrity checks for OMNI Security."""
from __future__ import annotations
import os
import psycopg

def main() -> int:
    dsn = os.environ.get("OMNI_DATABASE_URL")
    if not dsn:
        raise SystemExit("OMNI_DATABASE_URL is required")
    errors=[]
    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute("""SELECT COUNT(*) FROM core.entity_relationships r
                       WHERE NOT EXISTS (SELECT 1 FROM core.entities e WHERE e.id=r.source_entity_id)
                          OR NOT EXISTS (SELECT 1 FROM core.entities e WHERE e.id=r.target_entity_id)""")
        if cur.fetchone()[0]: errors.append("orphan entity relationships")
        cur.execute("""SELECT COUNT(*) FROM evidence.source_observations o
                       WHERE NOT EXISTS (SELECT 1 FROM source.sources s WHERE s.id=o.source_id)
                          OR NOT EXISTS (SELECT 1 FROM evidence.evidence e WHERE e.id=o.evidence_id)""")
        if cur.fetchone()[0]: errors.append("orphan source observations")
        cur.execute("""SELECT COUNT(*) FROM evidence.entity_evidence x
                       WHERE NOT EXISTS (SELECT 1 FROM core.entities e WHERE e.id=x.entity_id)
                          OR NOT EXISTS (SELECT 1 FROM evidence.evidence e WHERE e.id=x.evidence_id)""")
        if cur.fetchone()[0]: errors.append("orphan entity evidence")
        cur.execute("""SELECT COUNT(*) FROM evidence.observation_artifacts x
                       WHERE NOT EXISTS (SELECT 1 FROM evidence.source_observations o WHERE o.id=x.observation_id)
                          OR NOT EXISTS (SELECT 1 FROM evidence.artifacts a WHERE a.id=x.artifact_id)""")
        if cur.fetchone()[0]: errors.append("orphan observation/artifact links")
        cur.execute("""SELECT COUNT(*) FROM evidence.artifact_claims x
                       WHERE NOT EXISTS (SELECT 1 FROM evidence.artifacts a WHERE a.id=x.artifact_id)
                          OR NOT EXISTS (SELECT 1 FROM evidence.claims c WHERE c.id=x.claim_id)""")
        if cur.fetchone()[0]: errors.append("orphan artifact/claim links")
        cur.execute("""SELECT COUNT(*) FROM evidence.claim_entities x
                       WHERE NOT EXISTS (SELECT 1 FROM evidence.claims c WHERE c.id=x.claim_id)
                          OR NOT EXISTS (SELECT 1 FROM core.entities e WHERE e.id=x.entity_id)""")
        if cur.fetchone()[0]: errors.append("orphan claim/entity links")
        cur.execute("""SELECT COUNT(*) FROM source.sources s
                       WHERE s.publication_layer_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM taxonomy.publication_layers p WHERE p.id=s.publication_layer_id)""")
        if cur.fetchone()[0]: errors.append("sources reference unknown publication layers")
        cur.execute("""SELECT COUNT(*) FROM core.entities
                       WHERE confidence IS NOT NULL AND (confidence < 0 OR confidence > 1)""")
        if cur.fetchone()[0]: errors.append("invalid entity confidence")
    if errors:
        for e in errors: print(f"FAIL {e}")
        return 1
    print("PASS database integrity")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
