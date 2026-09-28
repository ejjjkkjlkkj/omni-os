import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GATE = ROOT / "scripts" / "coverage_gate.py"


def run(*args):
    return subprocess.run(
        [sys.executable, str(GATE), *args], capture_output=True, text=True
    )


class CoverageGateTests(unittest.TestCase):
    def test_no_argument_fails_closed_with_usage(self):
        proc = run()
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("usage", (proc.stdout + proc.stderr).lower())

    def test_missing_report_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            proc = run(str(Path(td) / "nope.txt"))
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("MISSING", proc.stdout + proc.stderr)

    def test_full_coverage_passes(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cov.txt"
            p.write_text("TOTAL 120 0 100.00% 45 0 100.00%\n", encoding="utf-8")
            proc = run(str(p))
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_incomplete_coverage_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cov.txt"
            p.write_text("TOTAL 120 6 95.00% 45 0 100.00%\n", encoding="utf-8")
            proc = run(str(p))
        self.assertNotEqual(proc.returncode, 0)


if __name__ == "__main__":
    unittest.main()
