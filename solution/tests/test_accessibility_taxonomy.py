import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TAX = ROOT / "data" / "taxonomy"
sys.path.insert(0, str(ROOT / "scripts"))
import build_accessibility_taxonomy as builder


class AccessibilityTaxonomyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.derived = builder.derive()

    def test_committed_files_match_the_derivation(self):
        # The committed taxonomy must be exactly what the generator derives from
        # the authoritative sources: no drift, no hand-edits, fully reproducible.
        for name, payload in self.derived.items():
            committed = json.loads((TAX / name).read_text(encoding="utf-8"))
            self.assertEqual(committed, payload, name)

    def test_depth_stack_is_23_layers_l16_to_lminus6(self):
        full = self.derived["full-stack-layers.json"]
        ids = [layer["id"] for layer in full["layers"]]
        self.assertEqual(len(ids), 23)
        self.assertEqual(ids[0], "L16")
        self.assertEqual(ids[-1], "L-6")

    def test_every_layer_exposes_the_full_accessibility_capability_set(self):
        a11y = self.derived["accessibility-full-stack.json"]
        caps = set(a11y["capabilities"])
        self.assertTrue(caps)
        for layer_id, meta in a11y["stack_layers"].items():
            self.assertEqual(set(meta["accessibility"]), caps, layer_id)
        for layer_id, meta in a11y["publication_layers"].items():
            self.assertEqual(set(meta["required"]), caps, layer_id)

    def test_publication_layers_match_network_planes(self):
        network = json.loads((TAX / "network-layers.json").read_text(encoding="utf-8"))
        a11y = self.derived["accessibility-full-stack.json"]
        self.assertEqual(
            set(a11y["publication_layers"]), set(network["publication_layers"])
        )

    def test_every_official_proof_has_concrete_evidence(self):
        official = self.derived["accessibility-official-source-proof.json"]["sources"]
        self.assertTrue(official)
        for sid, profile in official.items():
            self.assertTrue(profile["layers"], sid)
            self.assertTrue(profile["proof"], sid)

    def test_coverage_validators_pass_end_to_end(self):
        for script in (
            "check_accessibility_source_proof.py",
            "check_knowledge_coverage.py",
            "check_master_coverage.py",
        ):
            proc = subprocess.run(
                [sys.executable, f"scripts/{script}"],
                cwd=ROOT,
                capture_output=True,
                text=True,
            )
            self.assertEqual(proc.returncode, 0, f"{script}\n{proc.stdout}\n{proc.stderr}")


if __name__ == "__main__":
    unittest.main()
