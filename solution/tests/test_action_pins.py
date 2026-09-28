import json
import tempfile
import unittest
from pathlib import Path

from tools.verify_action_pins import verify


class ActionPinTests(unittest.TestCase):
    def test_all_external_actions_are_immutable_and_lock_bound(self):
        result = verify()
        self.assertEqual(result["status"], "PASS", result["violations"])
        self.assertGreater(result["checked"], 0)

    def _verify_fixture(self, workflow: str, actions: dict[str, str] | None = None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workflows = root / ".github" / "workflows"
            workflows.mkdir(parents=True)
            (workflows / "test.yml").write_text(workflow, encoding="utf-8")
            (root / "toolchains.lock.json").write_text(
                json.dumps({"github_actions": actions or {}}),
                encoding="utf-8",
            )
            return verify(root)

    def test_mutable_docker_reference_is_rejected(self):
        result = self._verify_fixture(
            "jobs:\n  x:\n    steps:\n      - uses: docker://ghcr.io/acme/tool:latest\n"
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "DOCKER_NOT_DIGEST_PINNED")

    def test_digest_pinned_docker_reference_is_allowed(self):
        digest = "a" * 64
        result = self._verify_fixture(
            f"jobs:\n  x:\n    steps:\n      - uses: docker://ghcr.io/acme/tool@sha256:{digest}\n"
        )
        self.assertEqual(result["status"], "PASS", result["violations"])

    def test_locked_action_must_use_exact_locked_sha(self):
        result = self._verify_fixture(
            f"jobs:\n  x:\n    steps:\n      - uses: actions/checkout@{'b' * 40}\n",
            {"checkout": "a" * 40},
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "ACTION_LOCK_MISMATCH")

    def test_untracked_external_action_is_rejected(self):
        result = self._verify_fixture(
            f"jobs:\n  x:\n    steps:\n      - uses: octo/example@{'a' * 40}\n"
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "ACTION_NOT_LOCKED")

    def test_flow_style_uses_cannot_bypass_pin_verifier(self):
        sha = "a" * 40
        result = self._verify_fixture(
            f"jobs: {{x: {{steps: [{{uses: actions/checkout@{sha}}}]}}}}\n",
            {"checkout": sha},
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(
            any(v["reason"] == "USES_SYNTAX_UNSUPPORTED" for v in result["violations"]),
            result,
        )

    def test_quoted_uses_key_cannot_bypass_pin_verifier(self):
        result = self._verify_fixture(
            'steps:\n  - "uses": actions/checkout@v4\n',
            {"checkout": "a" * 40},
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "USES_SYNTAX_UNSUPPORTED")

    def test_explicit_uses_key_is_rejected_fail_closed(self):
        result = self._verify_fixture(
            "? uses\n: actions/checkout@v4\n",
            {"checkout": "a" * 40},
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "USES_SYNTAX_UNSUPPORTED")

    def test_alias_mapping_key_is_rejected_fail_closed(self):
        result = self._verify_fixture(
            "policy_key: &policy_key uses\nsteps:\n  - *policy_key: actions/checkout@v4\n",
            {"checkout": "a" * 40},
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "USES_SYNTAX_UNSUPPORTED")

    def test_escaped_quoted_mapping_key_is_rejected_fail_closed(self):
        result = self._verify_fixture(
            'steps:\n  - "u\\u0073es": actions/checkout@v4\n',
            {"checkout": "a" * 40},
        )
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["violations"][0]["reason"], "USES_SYNTAX_UNSUPPORTED")

    def test_shell_json_in_block_scalar_is_not_a_yaml_key(self):
        result = self._verify_fixture(
            """steps:
  - run: |
      echo '{"schema":"omni.test.v1"}'
"""
        )
        self.assertEqual(result["status"], "PASS", result["violations"])

    def test_comment_uses_is_ignored(self):
        result = self._verify_fixture("# uses: actions/checkout@v4\n")
        self.assertEqual(result["status"], "PASS", result["violations"])


if __name__ == "__main__":
    unittest.main()
