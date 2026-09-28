import json
import pathlib
import tempfile
import unittest
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import evidence_ledger

class EvidenceLedgerTests(unittest.TestCase):
    def test_record_scrubs_secret_values(self):
        with tempfile.TemporaryDirectory() as td:
            root=pathlib.Path(td)
            original=evidence_ledger.AGENT; evidence_ledger.AGENT=root; evidence_ledger.LEDGER=root/"state/evidence.jsonl"
            try:
                result=evidence_ledger.record("verification","PASS","task",{"token":"VALUE","result":"ok"})
                self.assertEqual(result["status"],"PASS")
                line=evidence_ledger.LEDGER.read_text(encoding="utf-8")
                self.assertNotIn("VALUE",line)
                self.assertIn("[REDACTED]",line)
            finally:
                evidence_ledger.AGENT=original; evidence_ledger.LEDGER=original/"state/evidence.jsonl"

    def test_invalid_status_is_blocked(self):
        with tempfile.TemporaryDirectory() as td:
            root=pathlib.Path(td)
            original=evidence_ledger.AGENT; evidence_ledger.AGENT=root; evidence_ledger.LEDGER=root/"state/evidence.jsonl"
            try:
                self.assertEqual(evidence_ledger.record("x","PASSING","y")["status"],"BLOCKED")
            finally:
                evidence_ledger.AGENT=original; evidence_ledger.LEDGER=original/"state/evidence.jsonl"

if __name__ == "__main__":
    unittest.main()
