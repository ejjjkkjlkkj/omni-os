import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class DatabaseProvenanceTests(unittest.TestCase):
    def test_versioned_migrations_present_and_ordered(self):
        migrations = sorted((ROOT / "db" / "migrations").glob("*.sql"))
        self.assertTrue(migrations)
        self.assertEqual(migrations[0].name, "0001_initial_schema.sql")
        self.assertEqual([p.name for p in migrations], sorted(p.name for p in migrations))

    def test_provenance_chain_migration_declares_all_links(self):
        sql = (ROOT / "db" / "migrations" / "0002_provenance_chain.sql").read_text(encoding="utf-8")
        for table in (
            "evidence.observation_artifacts",
            "evidence.artifact_claims",
            "evidence.claim_entities",
        ):
            self.assertIn(f"CREATE TABLE {table}", sql)

    def test_importer_materializes_provenance_chain(self):
        sql = (ROOT / "scripts" / "db_import.py").read_text(encoding="utf-8")
        for token in (
            "evidence.source_observations",
            "evidence.artifacts",
            "evidence.observation_artifacts",
            "evidence.artifact_claims",
            "evidence.claim_entities",
        ):
            self.assertIn(token, sql)


if __name__ == "__main__":
    unittest.main()
