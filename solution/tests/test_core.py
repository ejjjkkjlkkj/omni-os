import tempfile, unittest
from pathlib import Path
from omni.semantic import SemanticModel
from omni.flight import build, verify
from omni.firmware import inspect
from omni.ceiling import REQUIRED_GATES, evaluate

class CoreTests(unittest.TestCase):
    def test_semantic_password_redaction(self):
        m = SemanticModel()
        m.apply({"sequence": 1, "kind": "node_created", "node": {"id": 1, "role": "edit", "name": "Password", "value": "secret", "state": ["password", "focusable"]}})
        self.assertFalse(m.passed)
        self.assertEqual(m.violations[0]["code"], "PASSWORD_VALUE_EXPOSED")

    def test_focus_unknown_node_is_reachable(self):
        m = SemanticModel()
        m.apply({"sequence": 1, "kind": "focus_changed", "node_id": 42})
        self.assertFalse(m.passed)
        self.assertEqual(m.violations[-1]["code"], "FOCUS_UNKNOWN_NODE")
        self.assertNotIn(42, m.nodes)

    def test_focus_does_not_erase_existing_semantics(self):
        m = SemanticModel()
        m.apply({"sequence": 1, "kind": "node_created", "node": {"id": 7, "role": "button", "name": "Boot", "state": ["enabled"]}})
        m.apply({"sequence": 2, "kind": "focus_changed", "node_id": 7})
        self.assertTrue(m.passed)
        self.assertEqual(m.nodes[7].role, "button")
        self.assertEqual(m.nodes[7].name, "Boot")
        self.assertIn("focused", m.nodes[7].state)

    def test_unknown_updates_and_events_fail_closed(self):
        m = SemanticModel()
        m.apply({"sequence": 1, "kind": "value_changed", "node": {"id": 9, "value": "x"}})
        m.apply({"sequence": 2, "kind": "future_event", "node_id": 9})
        self.assertEqual(
            [v["code"] for v in m.violations],
            ["VALUE_UNKNOWN_NODE", "UNKNOWN_EVENT_KIND"],
        )

    def test_flight_detects_tamper(self):
        records = build([{"layer": "uefi", "type": "focus"}, {"layer": "mm", "type": "ping"}])
        self.assertTrue(verify(records)[0])
        records[1]["event"]["type"] = "tampered"
        self.assertFalse(verify(records)[0])

    def test_firmware_read_only_probe(self):
        blob = bytearray(256)
        base = 32
        blob[base + 40:base + 44] = b"_FVH"
        blob[base + 32:base + 40] = (128).to_bytes(8, "little")
        blob[base + 48:base + 50] = (56).to_bytes(2, "little")
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "fw.bin"; p.write_bytes(blob)
            result = inspect(p)
            self.assertEqual(result["mutation"], False)
            self.assertEqual(len(result["firmware_volumes"]), 1)
            self.assertTrue(result["firmware_volumes"][0]["valid_bounds"])

    def test_ceiling_is_fail_closed(self):
        self.assertEqual(evaluate({})["status"], "SOFTWARE_INCOMPLETE")

        partial = {"core_semantics": "PASS", "native_verification": "PASS"}
        result = evaluate(partial)
        self.assertEqual(result["status"], "SOFTWARE_INCOMPLETE")
        self.assertTrue(result["missing"])

        complete = {gate: "PASS" for gate in REQUIRED_GATES}
        self.assertEqual(evaluate(complete)["status"], "SOFTWARE_CEILING_PASS")

        complete["uefi_sct_runtime_ovmf"] = "NOT_RUN"
        result = evaluate(complete)
        self.assertEqual(result["status"], "SOFTWARE_INCOMPLETE")
        self.assertIn("uefi_sct_runtime_ovmf", result["failed"])

    def test_ceiling_rejects_aliases_and_injected_gates(self):
        complete = {gate: "PASS" for gate in REQUIRED_GATES}
        complete["core_semantics"] = "pass"
        result = evaluate(complete)
        self.assertEqual(result["status"], "SOFTWARE_INCOMPLETE")
        self.assertIn("core_semantics", result["invalid"])

        complete["core_semantics"] = "PASS"
        complete["fake_gate"] = "PASS"
        result = evaluate(complete)
        self.assertEqual(result["status"], "SOFTWARE_INCOMPLETE")
        self.assertIn("fake_gate", result["unexpected"])

    def test_ceiling_rejects_non_string_status(self):
        with self.assertRaises(ValueError):
            evaluate({"core_semantics": 1})  # type: ignore[arg-type]

if __name__ == "__main__": unittest.main()
