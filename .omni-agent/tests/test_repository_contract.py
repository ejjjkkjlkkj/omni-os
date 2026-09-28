import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
KNOWLEDGE = ROOT / "knowledge"

REQUIRED_KNOWLEDGE = (
    "schema.json",
    "sources.json",
    "requirements.json",
    "coverage.json",
    "engineering-map.json",
    "release-gate.json",
    "execution-ladder.json",
    "source-registry.json",
    "security-accessibility-contract.json",
)


class RepositoryContractTests(unittest.TestCase):
    def load(self, name):
        with (KNOWLEDGE / name).open(encoding="utf-8") as fh:
            return json.load(fh)

    def test_complete_knowledge_contract_is_present(self):
        missing = [name for name in REQUIRED_KNOWLEDGE if not (KNOWLEDGE / name).is_file()]
        self.assertEqual(missing, [])

    def test_dimensions_are_consistent(self):
        schema = self.load("schema.json")
        coverage = self.load("coverage.json")
        release = self.load("release-gate.json")
        expected = set(schema["dimensions"])
        self.assertEqual(expected, set(coverage["evidence_requirements"]["required_dimensions"]))
        self.assertEqual(expected, set(release["required_dimensions"]))

    def test_blocking_states_are_fail_closed(self):
        schema = self.load("schema.json")
        coverage = self.load("coverage.json")
        release = self.load("release-gate.json")
        blocking = {"UNKNOWN", "MISSING", "PARTIAL", "REGRESSED", "BLOCKED", "ENVIRONMENT"}
        self.assertTrue(blocking.issubset(set(schema["states"])))
        self.assertTrue(blocking.issubset(set(coverage["evidence_requirements"]["required_states"])))
        self.assertTrue(blocking.issubset(set(release["blocking_states"])))

    def test_security_and_accessibility_have_independent_evidence(self):
        contract = self.load("security-accessibility-contract.json")
        matrix = contract["evidence_matrix"]
        self.assertTrue(matrix["security"])
        self.assertTrue(matrix["accessibility"])
        self.assertTrue(matrix["release"])

    def test_execution_is_a_complete_continuous_cycle(self):
        ladder = self.load("execution-ladder.json")
        gates = ladder["gates"]
        self.assertEqual(len(gates), 12)
        self.assertEqual([gate["id"] for gate in gates], list(range(1, 13)))
        cycle = ladder["continuous_cycle"]
        self.assertTrue(cycle["enabled"])
        self.assertEqual(cycle["sequence"], list(range(1, 13)))

    def test_source_provenance_is_resolvable(self):
        registry = self.load("source-registry.json")
        ids = {source["id"] for source in registry["sources"]}
        requirements = self.load("requirements.json")["requirements"]
        for requirement in requirements:
            for source_id in requirement.get("source_refs", []):
                self.assertIn(source_id, ids, requirement["id"])

    def test_release_gate_is_fail_closed(self):
        release = self.load("release-gate.json")
        self.assertTrue(release["fail_closed"])
        self.assertTrue(release["requirements"]["no_unverified_success_claims"])
        self.assertTrue(release["requirements"]["no_secrets"])

    def test_level8_contract_has_reproducible_evidence(self):
        engineering = self.load("engineering-map.json")
        contract = engineering["level8_evidence_contract"]
        for key in ("observable_task", "domain_knowledge", "demonstrated_skill", "reproducible_evidence"):
            self.assertTrue(contract.get(key), key)


if __name__ == "__main__":
    unittest.main()
