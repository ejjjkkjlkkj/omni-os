import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import coverage_validator


class CoverageValidatorTests(unittest.TestCase):
    def test_current_knowledge_contract_is_structurally_valid(self):
        self.assertEqual(coverage_validator.validate(), [])

    def test_missing_domain_fails_closed(self):
        original = coverage_validator.load

        def fake_load(name):
            value = original(name)
            if name == "engineering-map.json":
                value = dict(value)
                value["coverage_domains"] = []
            return value

        coverage_validator.load = fake_load
        try:
            errors = coverage_validator.validate()
            self.assertTrue(any("coverage domain" in item for item in errors))
        finally:
            coverage_validator.load = original


if __name__ == "__main__":
    unittest.main()
