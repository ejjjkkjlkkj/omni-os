from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_versioned_migrations_present_and_ordered():
    migrations = sorted((ROOT / "db" / "migrations").glob("*.sql"))
    assert migrations
    assert migrations[0].name == "0001_initial_schema.sql"
    assert [p.name for p in migrations] == sorted(p.name for p in migrations)


def test_provenance_chain_migration_declares_all_links():
    sql = (ROOT / "db" / "migrations" / "0002_provenance_chain.sql").read_text(encoding="utf-8")
    for table in (
        "evidence.observation_artifacts",
        "evidence.artifact_claims",
        "evidence.claim_entities",
    ):
        assert f"CREATE TABLE {table}" in sql


def test_importer_materializes_provenance_chain():
    sql = (ROOT / "scripts" / "db_import.py").read_text(encoding="utf-8")
    for token in (
        "evidence.source_observations",
        "evidence.artifacts",
        "evidence.observation_artifacts",
        "evidence.artifact_claims",
        "evidence.claim_entities",
    ):
        assert token in sql
