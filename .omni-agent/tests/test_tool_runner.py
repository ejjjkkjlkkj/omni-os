import pathlib
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import tool_runner


class ToolRunnerTests(unittest.TestCase):
    def _config(self, command="python -m unittest", **policy):
        catalog = {"tools": [{"id": "x", "command": command}]}
        defaults = {"allowed_commands": [command], "blocked_patterns": [], "max_output_bytes": 1000, "default_timeout_seconds": 5, "never_store": []}
        defaults.update(policy)
        return catalog, defaults

    def test_unknown(self):
        self.assertEqual(tool_runner.run_tool("missing")["status"], "UNKNOWN")

    def test_policy_block(self):
        catalog, policy = self._config("blocked-command", blocked_patterns=["blocked-command"])
        with patch.object(tool_runner, "_load", side_effect=[catalog, policy]):
            self.assertEqual(tool_runner.run_tool("x")["status"], "BLOCKED")

    def test_allowed_uses_argv_execution(self):
        catalog, policy = self._config()
        with patch.object(tool_runner, "_load", side_effect=[catalog, policy]), patch.object(tool_runner.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "ok"
            result = tool_runner.run_tool("x")
            self.assertEqual(result["status"], "PASS")
            self.assertFalse(run.call_args.kwargs["shell"])
            self.assertEqual(run.call_args.args[0], ["python", "-m", "unittest"])

    def test_argument_metacharacters_are_data(self):
        command = "python -m unittest & marker"
        catalog, policy = self._config(command)
        with patch.object(tool_runner, "_load", side_effect=[catalog, policy]), patch.object(tool_runner.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "ok"
            tool_runner.run_tool("x")
            self.assertEqual(run.call_args.args[0][-2:], ["&", "marker"])
            self.assertFalse(run.call_args.kwargs["shell"])

    def test_secret_scrubbing(self):
        catalog, policy = self._config(never_store=["token", "api_key"])
        with patch.object(tool_runner, "_load", side_effect=[catalog, policy]), patch.object(tool_runner.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "token=VALUE api_key:VALUE2"
            result = tool_runner.run_tool("x")
            self.assertNotIn("VALUE", result["output"])
            self.assertNotIn("VALUE2", result["output"])
            self.assertIn("REDACTED", result["output"])

    def test_output_is_bounded(self):
        catalog, policy = self._config(max_output_bytes=10)
        with patch.object(tool_runner, "_load", side_effect=[catalog, policy]), patch.object(tool_runner.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "0123456789abcdef"
            result = tool_runner.run_tool("x")
            self.assertTrue(result["truncated"])
            self.assertLessEqual(len(result["output"].encode("utf-8")), 10)

    def test_timeout_is_unknown(self):
        catalog, policy = self._config()
        with patch.object(tool_runner, "_load", side_effect=[catalog, policy]), patch.object(tool_runner.subprocess, "run", side_effect=tool_runner.subprocess.TimeoutExpired("x", 5)):
            self.assertEqual(tool_runner.run_tool("x")["status"], "UNKNOWN")

    def test_protected_and_traversal_paths_are_blocked(self):
        self.assertFalse(tool_runner.validate_write_path(".omni-agent/state/latest.json"))
        self.assertFalse(tool_runner.validate_write_path("../outside.txt"))


if __name__ == "__main__":
    unittest.main()
