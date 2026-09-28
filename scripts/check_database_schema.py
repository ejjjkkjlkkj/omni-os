#!/usr/bin/env python3
"""Static database contract checker for OMNI Security."""
from __future__ import annotations

import csv
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SQL = (ROOT / "db/migrations/0001_initial_schema.sql").read_text(encoding="utf-8")
MASTER = json.loads((ROOT / "data/taxonomy/master-coverage.json").read_text(encoding="utf-8"))
A11Y = json.loads((ROOT / "data/taxonomy/accessibility-full-stack.json").read_text(encoding="utf-8"))
SEARCH = json.loads((ROOT / "data/taxonomy/search-engine-coverage.json").read_text(encoding="utf-8"))
SOURCES_DOC = json.loads((ROOT / "data/threat-intel/sources.json").read_text(encoding="utf-8"))
SOURCES = SOURCES_DOC.get("sources", [])

EXPECTED_SCHEMAS = {
    "core","taxonomy","source","evidence","intel","accessibility","search",
    "infrastructure","supply_chain","identity","audit","ingestion","analytics"
}
EXPECTED_TABLES = {
    "core.entities","core.entity_relationships","source.sources",
    "evidence.evidence","evidence.source_observations","evidence.claims",
    "intel.objects","intel.indicators","accessibility.requirements",
    "accessibility.screen_readers","accessibility.validation_runs",
    "accessibility.test_cases","accessibility.test_results",
    "accessibility.entity_layer_requirements","search.engines",
    "search.code_sources","search.runtime_validations","search.query_runs",
    "search.results","infrastructure.firmware","infrastructure.uefi_components",
    "infrastructure.boot_measurements","infrastructure.attestations",
    "infrastructure.root_of_trust","supply_chain.dependencies",
    "supply_chain.sbom","supply_chain.attestations","supply_chain.signatures",
    "identity.identities","identity.credentials","audit.events",
    "audit.validation_runs","audit.validation_results","ingestion.jobs",
    "ingestion.raw_artifacts","ingestion.normalization_runs","ingestion.errors",
}

def main() -> int:
    errors: list[str] = []
    declared_schemas = set(re.findall(r"CREATE SCHEMA IF NOT EXISTS ([a-z_]+)", SQL))
    declared_tables = set(re.findall(r"CREATE TABLE ([a-z_]+\.[a-z_]+)", SQL))
    errors.extend(f"missing schema: {x}" for x in sorted(EXPECTED_SCHEMAS - declared_schemas))
    errors.extend(f"missing table: {x}" for x in sorted(EXPECTED_TABLES - declared_tables))

    stack = A11Y.get("stack_layers", {})
    publication = A11Y.get("publication_layers", {})
    if not stack:
        errors.append("accessibility stack taxonomy is empty")
    if not publication:
        errors.append("accessibility publication taxonomy is empty")

    engine_ids = [x["id"] for x in SEARCH.get("engines", [])]
    if len(engine_ids) != len(set(engine_ids)):
        errors.append("duplicate search engine IDs")
    if not engine_ids:
        errors.append("search engine taxonomy is empty")

    domain_ids = [x["id"] for x in MASTER.get("domains", [])]
    if len(domain_ids) != len(set(domain_ids)):
        errors.append("duplicate master domain IDs")
    if not domain_ids:
        errors.append("master coverage domain taxonomy is empty")

    if not A11Y.get("stack_layers"):
        errors.append("accessibility stack layer taxonomy is empty")
    if not A11Y.get("publication_layers"):
        errors.append("accessibility publication layer taxonomy is empty")

    source_ids = [x["id"] for x in SOURCES]
    if len(source_ids) != len(set(source_ids)):
        errors.append("duplicate source IDs")

    with (ROOT / "requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv").open(
        newline="", encoding="utf-8"
    ) as fh:
        rows = list(csv.DictReader(fh))
    required_columns = {
        "id","domain","requirement","security_property","accessibility_property",
        "required_evidence","status"
    }
    if not rows:
        errors.append("accessibility/security requirements CSV is empty")
    if rows and not required_columns.issubset(rows[0]):
        errors.append("requirements CSV columns incomplete")

    # The schema must explicitly represent the co-equal security/accessibility contract.
    if "accessibility.entity_layer_requirements" not in declared_tables:
        errors.append("missing first-class accessibility layer mapping")
    if "evidence.entity_evidence" not in declared_tables:
        errors.append("missing evidence/entity provenance mapping")

    if errors:
        for error in errors:
            print(f"FAIL {error}")
        return 1

    print(
        "PASS database contract: "
        f"{len(declared_schemas)} schemas, {len(declared_tables)} tables, "
        f"{len(stack)} stack layers, {len(publication)} publication layers, "
        f"{len(engine_ids)} search entries, {len(source_ids)} configured sources, "
        f"{len(rows)} security/accessibility requirements"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
