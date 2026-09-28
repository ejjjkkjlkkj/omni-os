import math
import unittest

from omni.flight import FlightError, build, canonical, verify


class FlightRecorderTests(unittest.TestCase):
    def test_empty_chain_is_not_evidence(self):
        self.assertEqual(verify([]), (False, "EMPTY_CHAIN"))
        self.assertTrue(verify([], require_non_empty=False)[0])

    def test_non_finite_numbers_are_rejected(self):
        with self.assertRaises(FlightError):
            canonical({"value": math.nan})
        with self.assertRaises(FlightError):
            canonical({"value": math.inf})

    def test_unsupported_json_types_are_rejected(self):
        with self.assertRaises(FlightError):
            canonical({"value": b"not-json"})  # type: ignore[dict-item]

    def test_malformed_records_fail_without_exception(self):
        cases = [
            [None],
            [{"scheme": "omni.flight.v1"}],
            [
                {
                    "scheme": "omni.flight.v1",
                    "index": 0,
                    "prev_hash": "not-a-hash",
                    "event": {},
                    "hash": "0" * 64,
                }
            ],
            [
                {
                    "scheme": "omni.flight.v1",
                    "index": 0,
                    "prev_hash": "0" * 64,
                    "event": {"value": math.nan},
                    "hash": "0" * 64,
                }
            ],
        ]
        for records in cases:
            with self.subTest(records=records):
                passed, _ = verify(records)
                self.assertFalse(passed)

    def test_scheme_is_committed_and_required(self):
        records = build([{"kind": "focus", "node": 1}])
        self.assertTrue(verify(records)[0])
        records[0]["scheme"] = "other"
        self.assertEqual(verify(records), (False, "SCHEME"))

    def test_hash_tamper_is_detected(self):
        records = build(
            [
                {"kind": "focus", "node": 1},
                {"kind": "value", "node": 1, "value": "Disabled"},
            ]
        )
        self.assertTrue(verify(records)[0])
        records[1]["event"]["value"] = "Enabled"  # type: ignore[index]
        self.assertEqual(verify(records), (False, "HASH"))


if __name__ == "__main__":
    unittest.main()
