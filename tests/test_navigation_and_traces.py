import unittest

from omni.navgraph import check_graph
from omni.traces import consensus, diff


class NavigationAndTraceTests(unittest.TestCase):
    def setUp(self):
        self.graph = {
            "start": "root",
            "nodes": [
                {
                    "id": "root",
                    "name": "Setup",
                    "edges": [{"action": "enter", "to": "boot"}],
                },
                {
                    "id": "boot",
                    "name": "Boot",
                    "focusable": True,
                    "edges": [
                        {"action": "down", "to": "secure"},
                        {"action": "back", "to": "root"},
                    ],
                },
                {
                    "id": "secure",
                    "name": "Secure Boot",
                    "value": "Enabled",
                    "focusable": True,
                    "edges": [
                        {"action": "up", "to": "boot"},
                        {"action": "back", "to": "root"},
                    ],
                },
            ],
        }
        self.trace = [
            {
                "kind": "focus_changed",
                "node": {
                    "role": "checkbox",
                    "name": "Secure Boot",
                    "value": "Enabled",
                    "state": ["focused", "enabled"],
                },
            },
            {
                "kind": "value_changed",
                "node": {
                    "role": "checkbox",
                    "name": "Secure Boot",
                    "value": "Disabled",
                    "state": ["focused", "enabled"],
                },
            },
        ]

    def test_graph_passes(self):
        result = check_graph(self.graph)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["coverage_percent"], 100.0)

    def test_graph_detects_nondeterminism(self):
        self.graph["nodes"][1]["edges"].append({"action": "down", "to": "root"})
        result = check_graph(self.graph)
        self.assertEqual(result["status"], "FAIL")
        self.assertIn(
            "NONDETERMINISTIC_ACTION",
            {item["code"] for item in result["violations"]},
        )

    def test_graph_rejects_duplicate_ids(self):
        self.graph["nodes"].append(
            {"id": "boot", "name": "Duplicate", "focusable": True, "edges": []}
        )
        result = check_graph(self.graph)
        self.assertEqual(result["status"], "FAIL")
        self.assertIn(
            "DUPLICATE_NODE_ID",
            {item["code"] for item in result["violations"]},
        )

    def test_graph_rejects_malformed_nodes_and_edges(self):
        malformed = {
            "start": "root",
            "nodes": [
                {"id": "root", "edges": [{"to": "x"}, "bad-edge"]},
                "bad-node",
                {"id": "x", "terminal": True, "edges": []},
            ],
        }
        result = check_graph(malformed)
        codes = {item["code"] for item in result["violations"]}
        self.assertEqual(result["status"], "FAIL")
        self.assertIn("EDGE_MISSING_ACTION", codes)
        self.assertIn("EDGE_NOT_OBJECT", codes)
        self.assertIn("NODE_NOT_OBJECT", codes)

    def test_graph_rejects_non_list_nodes(self):
        result = check_graph({"start": "root", "nodes": "not-a-list"})
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["coverage_percent"], 0.0)
        self.assertIn(
            "NODES_NOT_LIST",
            {item["code"] for item in result["violations"]},
        )

    def test_trace_consensus(self):
        other = [
            {
                "kind": "focus_changed",
                "timestamp": 999,
                "node": {
                    "id": 400,
                    "role": "CHECKBOX",
                    "name": " Secure   Boot ",
                    "value": "Enabled",
                    "state": ["enabled", "focused"],
                },
            },
            {
                "kind": "value_changed",
                "node": {
                    "id": 400,
                    "role": "checkbox",
                    "name": "Secure Boot",
                    "value": "Disabled",
                    "state": ["focused", "enabled"],
                },
            },
        ]
        self.assertEqual(diff(self.trace, other)["status"], "PASS")
        self.assertEqual(consensus([self.trace, other, self.trace])["status"], "PASS")

    def test_vacuous_trace_proofs_fail_closed(self):
        self.assertEqual(diff([], [])["status"], "FAIL")
        self.assertEqual(diff([], self.trace)["reason"], "EMPTY_TRACE")
        self.assertEqual(consensus([])["reason"], "NO_TRACES")
        self.assertEqual(consensus([self.trace])["reason"], "INSUFFICIENT_TRACES")
        self.assertEqual(consensus([[], []])["reason"], "EMPTY_TRACE")
        self.assertEqual(consensus([self.trace, []])["status"], "FAIL")

    def test_trace_consensus_rejects_malformed_event(self):
        result = consensus([[{"kind": "focus_changed", "node": []}], self.trace])
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["reason"], "INVALID_EVENT")

    def test_trace_diff_rejects_malformed_event(self):
        result = diff([{"kind": "focus_changed", "node": []}], self.trace)
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["reason"], "INVALID_EVENT")

    def test_unhashable_kind_fails_closed_in_both(self):
        # An unhashable "kind" must be a clean INVALID_EVENT, not an uncaught
        # TypeError inside the consensus tally.
        bad = [{"kind": ["not", "hashable"], "node": {"role": "form"}}]
        for result in (consensus([bad, self.trace]), diff(bad, self.trace)):
            self.assertEqual(result["status"], "FAIL")
            self.assertEqual(result["reason"], "INVALID_EVENT")


if __name__ == "__main__":
    unittest.main()
