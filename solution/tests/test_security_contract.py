import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "data" / "schema" / "security-knowledge.schema.json"


class SecurityContractTests(unittest.TestCase):
    def test_security_knowledge_schema_is_a_valid_static_contract(self):
        # The schema is a hand-authored static artifact (never network-generated);
        # it must exist and describe the generated cache the updater emits.
        self.assertTrue(SCHEMA.is_file(), "security-knowledge schema is missing")
        schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        for key in ("$schema", "title", "type", "required", "properties"):
            self.assertIn(key, schema)
        self.assertEqual(schema["type"], "object")
        for prop in ("schema_version", "personal_data_policy", "record_count", "records"):
            self.assertIn(prop, schema["properties"])
        # The records shape must require source-attribution and a normalized type.
        item = schema["properties"]["records"]["items"]
        self.assertEqual(set(item["required"]), {"source", "entity_type"})

    def test_security_contract_validator_passes_fail_closed(self):
        # Runs the repository's own fail-closed validator end-to-end. An empty or
        # not-yet-generated intelligence cache must be tolerated (updater CI fills
        # it), while every static contract must validate.
        proc = subprocess.run(
            [sys.executable, "scripts/validate_security_contract.py"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertNotIn("ERROR:", proc.stdout)
        self.assertIn("security contract validation complete", proc.stdout)


if __name__ == "__main__":
    unittest.main()
