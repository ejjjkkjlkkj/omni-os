#!/usr/bin/env python3
"""Runtime coverage contract checks for OMNI Security."""
from __future__ import annotations
import os
import psycopg

def main() -> int:
    dsn=os.environ.get("OMNI_DATABASE_URL")
    if not dsn: raise SystemExit("OMNI_DATABASE_URL is required")
    errors=[]
    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute("SELECT COUNT(*) FROM taxonomy.domains")
        domains=cur.fetchone()[0]
        cur.execute("SELECT COUNT(*) FROM taxonomy.stack_layers")
        stacks=cur.fetchone()[0]
        cur.execute("SELECT COUNT(*) FROM taxonomy.publication_layers")
        pubs=cur.fetchone()[0]
        if domains == 0: errors.append("taxonomy.domains is empty")
        if stacks < 23: errors.append(f"expected at least 23 stack layers, got {stacks}")
        if pubs < 21: errors.append(f"expected at least 21 publication layers, got {pubs}")
        cur.execute("""SELECT COUNT(*) FROM source.sources
                       WHERE required AND status IN ('unverified','failed','blocked','stale')""")
        bad_required=cur.fetchone()[0]
        if bad_required: errors.append(f"{bad_required} required sources are not validated")
        cur.execute("""SELECT COUNT(*) FROM source.sources
                       WHERE publication_layer_id IS NULL""")
        if cur.fetchone()[0]: errors.append("sources missing publication layer")
    if errors:
        for e in errors: print(f"FAIL {e}")
        return 1
    print("PASS database coverage")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
