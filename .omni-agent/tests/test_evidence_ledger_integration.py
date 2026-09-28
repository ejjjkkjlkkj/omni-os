import json
import pathlib
import tempfile
import unittest

import sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import evidence_ledger


class EvidenceLedgerIntegrationTests(unittest.TestCase):
    def test_record_scrubs_secrets_and_emits_hash(self):
        with tempfile.TemporaryDirectory() as td:
            old = evidence_ledger.LEDGER
            try:
                evidence_ledger.LEDGER = pathlib.Path(td) / "evidence.jsonl"
                result = evidence_ledger.record(
                    "verification",
                    "PASS",
                    "cycle",
                    {"api_key": "SHOULD_NOT_APPEAR", "result": "ok"},
                )
                self.assertEqual(result["status"], "PASS")
                line = evidence_ledger.LEDGER.read_text(encoding="utf-8").strip()
                self.assertNotIn("SHOULD_NOT_APPEAR", line)
                row = json.loads(line)
                self.assertEqual(len(row["record_sha256"]), 64)
                self.assertIn("[REDACTED]", row["details"])
            finally:
                evidence_ledger.LEDGER = old


if __name__ == "__main__":
    unittest.main()
