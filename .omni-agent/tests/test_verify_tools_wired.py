import json
import pathlib
import unittest

AGENT = pathlib.Path(__file__).resolve().parents[1]

# Verify tools the autonomous cycle relies on to prove a dimension. Each must
# have an executable, allowlisted command so the agent can actually reach
# verified=True instead of stalling on FAIL/UNKNOWN.
REQUIRED_VERIFY_TOOLS = {"compile.python", "test.python", "security.audit", "accessibility.audit"}


class VerifyToolsWiredTests(unittest.TestCase):
    def setUp(self):
        self.tools = json.loads((AGENT / "tools.json").read_text(encoding="utf-8"))["tools"]
        self.policy = json.loads((AGENT / "tool_policy.json").read_text(encoding="utf-8"))
        self.allowed = self.policy["allowed_commands"]

    def _allowlisted(self, cmd):
        return any(cmd == a or cmd.startswith(a + " ") for a in self.allowed)

    def test_required_verify_tools_have_allowlisted_commands(self):
        by_id = {t["id"]: t for t in self.tools}
        for tid in REQUIRED_VERIFY_TOOLS:
            self.assertIn(tid, by_id, tid)
            cmd = by_id[tid].get("command")
            self.assertTrue(cmd, f"{tid} has no executable command")
            self.assertTrue(self._allowlisted(cmd), f"{tid} command not allowlisted: {cmd}")

    def test_python_test_tool_uses_the_projects_runner(self):
        # The project is unittest-based (no pytest dependency); the tool must not
        # silently require an uninstalled runner.
        cmd = next(t["command"] for t in self.tools if t["id"] == "test.python")
        self.assertIn("unittest", cmd)
        self.assertNotIn("pytest", cmd)


if __name__ == "__main__":
    unittest.main()
