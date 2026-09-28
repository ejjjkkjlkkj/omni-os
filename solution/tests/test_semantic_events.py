import unittest

from omni.semantic import SemanticModel


class SemanticEventStateMachineTests(unittest.TestCase):
    def make_model(self) -> SemanticModel:
        model = SemanticModel()
        model.apply(
            {
                "sequence": 1,
                "kind": "node_created",
                "node": {
                    "id": 10,
                    "role": "checkbox",
                    "name": "Secure Boot",
                    "value": "Enabled",
                    "state": ["focusable", "enabled"],
                },
            }
        )
        return model

    def test_unknown_focus_is_rejected_without_creating_node(self):
        model = self.make_model()
        model.apply(
            {
                "sequence": 2,
                "kind": "focus_changed",
                "node_id": 99,
            }
        )
        self.assertNotIn(99, model.nodes)
        self.assertIsNone(model.focus)
        self.assertIn(
            "FOCUS_UNKNOWN_NODE",
            {item["code"] for item in model.violations},
        )

    def test_partial_value_change_preserves_metadata(self):
        model = self.make_model()
        model.apply(
            {
                "sequence": 2,
                "kind": "value_changed",
                "node": {"id": 10, "value": "Disabled"},
            }
        )
        node = model.nodes[10]
        self.assertEqual(node.role, "checkbox")
        self.assertEqual(node.name, "Secure Boot")
        self.assertEqual(node.value, "Disabled")
        self.assertEqual(node.state, {"focusable", "enabled"})

    def test_partial_node_update_preserves_omitted_fields(self):
        model = self.make_model()
        model.apply(
            {
                "sequence": 2,
                "kind": "node_updated",
                "node": {"id": 10, "name": "Secure Boot Control"},
            }
        )
        node = model.nodes[10]
        self.assertEqual(node.name, "Secure Boot Control")
        self.assertEqual(node.role, "checkbox")
        self.assertEqual(node.value, "Enabled")
        self.assertEqual(node.state, {"focusable", "enabled"})

    def test_duplicate_create_does_not_overwrite_existing_state(self):
        model = self.make_model()
        model.apply(
            {
                "sequence": 2,
                "kind": "node_created",
                "node": {
                    "id": 10,
                    "role": "button",
                    "name": "Overwritten",
                },
            }
        )
        self.assertEqual(model.nodes[10].role, "checkbox")
        self.assertEqual(model.nodes[10].name, "Secure Boot")
        self.assertIn(
            "DUPLICATE_NODE_CREATE",
            {item["code"] for item in model.violations},
        )

    def test_focus_event_with_id_only_preserves_semantics(self):
        model = self.make_model()
        model.apply(
            {
                "sequence": 2,
                "kind": "focus_changed",
                "node_id": 10,
            }
        )
        node = model.nodes[10]
        self.assertEqual(model.focus, 10)
        self.assertEqual(node.role, "checkbox")
        self.assertEqual(node.name, "Secure Boot")
        self.assertEqual(node.value, "Enabled")
        self.assertIn("focused", node.state)

    def test_unknown_mutations_are_rejected(self):
        model = self.make_model()
        model.apply(
            {
                "sequence": 2,
                "kind": "value_changed",
                "node": {"id": 77, "value": "x"},
            }
        )
        model.apply(
            {
                "sequence": 3,
                "kind": "state_changed",
                "node": {"id": 78, "state": ["enabled"]},
            }
        )
        codes = {item["code"] for item in model.violations}
        self.assertIn("VALUE_UNKNOWN_NODE", codes)
        self.assertIn("STATE_UNKNOWN_NODE", codes)
        self.assertNotIn(77, model.nodes)
        self.assertNotIn(78, model.nodes)


if __name__ == "__main__":
    unittest.main()
