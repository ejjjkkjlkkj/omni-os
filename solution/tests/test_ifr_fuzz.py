import unittest

from omni.ifr import parse_hii_package_list
from tools.ifr_fuzz import run, valid_seed


class IfrFuzzTests(unittest.TestCase):
    def test_seed_is_valid(self):
        result = parse_hii_package_list(valid_seed())
        self.assertEqual(len(result["packages"]), 1)
        self.assertEqual(len(result["packages"][0]["ifr"]), 2)

    def test_deterministic_fuzz_smoke(self):
        result = run(2000, 0x12345678)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["cases"], 2000)
        self.assertEqual(result["accepted"] + result["rejected"], 2000)


if __name__ == "__main__":
    unittest.main()
