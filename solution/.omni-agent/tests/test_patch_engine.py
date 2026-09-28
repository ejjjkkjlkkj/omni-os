import hashlib
import json
import pathlib
import tempfile
import unittest

import sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import patch_engine

class PatchEngineTests(unittest.TestCase):
    def test_exact_patch_and_checkpoint(self):
        with tempfile.TemporaryDirectory() as td:
            root = pathlib.Path(td)
            target = root / "sample.txt"
            target.write_text("alpha\nbeta\n", encoding="utf-8")
            original_root = patch_engine.ROOT
            original_agent = patch_engine.AGENT
            patch_engine.ROOT = root
            patch_engine.AGENT = root / ".omni-agent"
            patch_engine.AGENT.mkdir()
            (patch_engine.AGENT / "state").mkdir()
            (patch_engine.AGENT / "tool_policy.json").write_text(json.dumps({"protected_paths":[".git"]}), encoding="utf-8")
            (patch_engine.AGENT / "config.json").write_text(json.dumps({"max_file_bytes":1000}), encoding="utf-8")
            try:
                raw = target.read_bytes()
                result = patch_engine.apply_patch({"task_id":"t1","path":"sample.txt","expected_sha256":hashlib.sha256(raw).hexdigest(),"old_text":"beta","new_text":"gamma","max_replacements":1,"require_exact_replacements":True})
                self.assertEqual(result["status"], "PASS")
                self.assertEqual(target.read_text(encoding="utf-8"), "alpha\ngamma\n")
                self.assertTrue((patch_engine.AGENT/"state/checkpoints/t1.json").is_file())
            finally:
                patch_engine.ROOT, patch_engine.AGENT = original_root, original_agent

    def test_checkpoint_task_id_and_replay_are_blocked(self):
        with tempfile.TemporaryDirectory() as td:
            root=pathlib.Path(td); target=root/"x.txt"; target.write_text("x", encoding="utf-8")
            patch_engine.ROOT=root; patch_engine.AGENT=root/".omni-agent"; patch_engine.AGENT.mkdir()
            (patch_engine.AGENT/"tool_policy.json").write_text(json.dumps({"protected_paths":[]}), encoding="utf-8")
            (patch_engine.AGENT/"config.json").write_text(json.dumps({"max_file_bytes":1000}), encoding="utf-8")
            sha=hashlib.sha256(target.read_bytes()).hexdigest()
            spec={"task_id":"../escape","path":"x.txt","expected_sha256":sha,"old_text":"x","new_text":"y"}
            self.assertEqual(patch_engine.apply_patch(spec)["status"], "UNKNOWN")
            spec["task_id"]="t-replay"; first=patch_engine.apply_patch(spec)
            self.assertEqual(first["status"], "PASS")
            target.write_text("x", encoding="utf-8")
            self.assertEqual(patch_engine.apply_patch(spec)["status"], "UNKNOWN")

    def test_hash_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            root=pathlib.Path(td); target=root/"x.txt"; target.write_text("x", encoding="utf-8")
            patch_engine.ROOT=root; patch_engine.AGENT=root/".omni-agent"; patch_engine.AGENT.mkdir()
            (patch_engine.AGENT/"tool_policy.json").write_text(json.dumps({"protected_paths":[]}), encoding="utf-8")
            (patch_engine.AGENT/"config.json").write_text(json.dumps({"max_file_bytes":1000}), encoding="utf-8")
            self.assertEqual(patch_engine.apply_patch({"task_id":"t2","path":"x.txt","expected_sha256":"0"*64,"old_text":"x","new_text":"y"})["status"],"FAIL")

    def test_multiple_matches_fail(self):
        with tempfile.TemporaryDirectory() as td:
            root=pathlib.Path(td); target=root/"x.txt"; target.write_text("x x", encoding="utf-8")
            patch_engine.ROOT=root; patch_engine.AGENT=root/".omni-agent"; patch_engine.AGENT.mkdir()
            (patch_engine.AGENT/"tool_policy.json").write_text(json.dumps({"protected_paths":[]}), encoding="utf-8")
            (patch_engine.AGENT/"config.json").write_text(json.dumps({"max_file_bytes":1000}), encoding="utf-8")
            sha=hashlib.sha256(target.read_bytes()).hexdigest()
            self.assertEqual(patch_engine.apply_patch({"task_id":"t3","path":"x.txt","expected_sha256":sha,"old_text":"x","new_text":"y","max_replacements":1,"require_exact_replacements":True})["status"],"FAIL")

    def test_checkpoint_payload_contains_no_patch_content(self):
        with tempfile.TemporaryDirectory() as td:
            root=pathlib.Path(td); target=root/"x.txt"; target.write_text("secret-old", encoding="utf-8")
            patch_engine.ROOT=root; patch_engine.AGENT=root/".omni-agent"; patch_engine.AGENT.mkdir()
            (patch_engine.AGENT/"tool_policy.json").write_text(json.dumps({"protected_paths":[]}), encoding="utf-8")
            (patch_engine.AGENT/"config.json").write_text(json.dumps({"max_file_bytes":1000}), encoding="utf-8")
            sha=hashlib.sha256(target.read_bytes()).hexdigest()
            result=patch_engine.apply_patch({"task_id":"t-secret","path":"x.txt","expected_sha256":sha,"old_text":"secret-old","new_text":"secret-new"})
            self.assertEqual(result["status"], "PASS")
            checkpoint=json.loads((patch_engine.AGENT/"state/checkpoints/t-secret.json").read_text(encoding="utf-8"))
            self.assertNotIn("old_text", checkpoint)
            self.assertNotIn("new_text", checkpoint)
            self.assertNotIn("secret-old", json.dumps(checkpoint))
            self.assertNotIn("secret-new", json.dumps(checkpoint))

if __name__ == "__main__":
    unittest.main()
