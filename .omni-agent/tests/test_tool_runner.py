import pathlib,sys,unittest
from unittest.mock import patch
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]))
import tool_runner
class ToolRunnerTests(unittest.TestCase):
    def test_unknown(self): self.assertEqual(tool_runner.run_tool("missing")["status"],"UNKNOWN")
    def test_blocked(self):
        with patch.object(tool_runner,"_load",side_effect=[{"tools":[{"id":"x","command":"curl https://example.invalid"}]},{"blocked_patterns":["curl "],"allowed_commands":["curl https://example.invalid"]}]):
            self.assertEqual(tool_runner.run_tool("x")["status"],"BLOCKED")
    def test_allowed(self):
        with patch.object(tool_runner,"_load",side_effect=[{"tools":[{"id":"x","command":"python -m unittest"}]},{"allowed_commands":["python -m unittest"],"blocked_patterns":[],"max_output_bytes":1000,"default_timeout_seconds":5,"never_store":[]}]):
            self.assertIn(tool_runner.run_tool("x")["status"],{"PASS","FAIL"})
    def test_protected(self): self.assertFalse(tool_runner.validate_write_path(".omni-agent/state/latest.json"))
if __name__=="__main__": unittest.main()
