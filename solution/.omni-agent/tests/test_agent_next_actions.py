import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import agent


class AgentNextActionsTests(unittest.TestCase):
    def test_missing_security_source_emits_p0_restore_action(self):
        # Fail-closed: an unavailable security source must produce the P0
        # restore-security-source action, not be silently downgraded.
        actions = agent.next_actions({"python": True}, True, False, {})
        ids = [a["id"] for a in actions]
        self.assertIn("restore-security-source", ids)
        restore = next(a for a in actions if a["id"] == "restore-security-source")
        self.assertEqual(restore["priority"], "P0")

    def test_available_security_source_has_no_restore_action(self):
        actions = agent.next_actions({"python": True}, True, True, {})
        self.assertNotIn("restore-security-source", [a["id"] for a in actions])

    def test_incomplete_knowledge_emits_p0_complete_action(self):
        actions = agent.next_actions({"python": True}, False, True, {})
        self.assertIn("complete-knowledge-base", [a["id"] for a in actions])

    def test_failed_tool_result_emits_fix_action(self):
        actions = agent.next_actions(
            {"python": True}, True, True, {"test.python": {"status": "FAIL"}}
        )
        self.assertIn("fix-python-tests", [a["id"] for a in actions])


if __name__ == "__main__":
    unittest.main()
