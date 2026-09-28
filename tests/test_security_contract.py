#!/usr/bin/env python3
"""Minimal regression tests for the repository security contract."""
from __future__ import annotations

import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

def run(script: str) -> None:
    result = subprocess.run(
        [sys.executable, str(ROOT / script)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
    assert result.returncode == 0, f"{script} failed with exit code {result.returncode}"

class SecurityContractTests(unittest.TestCase):
    def test_security_contract(self) -> None:
    run("scripts/validate_security_contract.py")

    def test_master_coverage(self) -> None:
    run("scripts/check_master_coverage.py")

    def test_knowledge_coverage(self) -> None:
    run("scripts/check_knowledge_coverage.py")

    def test_accessibility_source_proof(self) -> None:
    run("scripts/check_accessibility_source_proof.py")

if __name__ == "__main__":
    unittest.main(verbosity=2)
