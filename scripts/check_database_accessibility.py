#!/usr/bin/env python3
"""Runtime accessibility coverage checks for OMNI Security."""
from __future__ import annotations
import os
import psycopg

def main() -> int:
    dsn=os.environ.get("OMNI_DATABASE_URL")
    if not dsn: raise SystemExit("OMNI_DATABASE_URL is required")
    errors=[]
    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute("SELECT COUNT(*) FROM accessibility.requirements")
        if cur.fetchone()[0] == 0: errors.append("accessibility requirements are empty")
        cur.execute("SELECT COUNT(*) FROM accessibility.screen_readers")
        if cur.fetchone()[0] < 7: errors.append("canonical screen-reader set incomplete")
        cur.execute("SELECT COUNT(*) FROM accessibility.entity_layer_requirements")
        mapped=cur.fetchone()[0]
        if mapped == 0:
            print("INFO no entity-layer accessibility validations exist yet; this is unvalidated coverage, not proof of support")
        cur.execute("SELECT COUNT(*) FROM taxonomy.stack_layers")
        layers=cur.fetchone()[0]
        cur.execute("SELECT COUNT(DISTINCT stack_layer_id) FROM accessibility.entity_layer_requirements")
        covered=cur.fetchone()[0]
        if mapped and covered < layers:
            errors.append("accessibility entity-layer coverage is incomplete")
    if errors:
        for e in errors: print(f"FAIL {e}")
        return 1
    print("PASS accessibility contract")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
