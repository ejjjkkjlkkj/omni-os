import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from contract_io import require_json


class ContractIoTests(unittest.TestCase):
    def _run(self, path):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            with self.assertRaises(SystemExit) as ctx:
                require_json(path, kind="accessibility taxonomy")
        self.assertEqual(ctx.exception.code, 1)
        return buf.getvalue()

    def test_absent_input_fails_closed_with_missing_reason(self):
        with tempfile.TemporaryDirectory() as td:
            out = self._run(Path(td) / "nope.json")
        self.assertIn("MISSING", out)
        self.assertIn("accessibility taxonomy", out)

    def test_empty_input_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "empty.json"
            p.write_text("", encoding="utf-8")
            out = self._run(p)
        self.assertIn("MISSING", out)

    def test_malformed_input_fails_closed_with_invalid_reason(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "bad.json"
            p.write_text("{not json", encoding="utf-8")
            out = self._run(p)
        self.assertIn("INVALID", out)

    def test_valid_input_is_returned(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "ok.json"
            p.write_text(json.dumps({"a": 1}), encoding="utf-8")
            self.assertEqual(require_json(p), {"a": 1})


if __name__ == "__main__":
    unittest.main()
