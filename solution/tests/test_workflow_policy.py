import tempfile
import unittest
from pathlib import Path

from tools.workflow_policy import inspect


class WorkflowPolicyTests(unittest.TestCase):
    def scan(self, text: str):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "ci.yml").write_text(text, encoding="utf-8")
            return inspect(root)

    def test_commit_pinned_action_passes(self):
        sha = "a" * 40
        self.assertEqual(self.scan(f"steps:\n  - uses: actions/checkout@{sha} # v4\n"), [])

    def test_moving_tag_is_rejected(self):
        violations = self.scan("steps:\n  - uses: actions/checkout@v4\n")
        self.assertEqual(violations[0].code, "ACTION_NOT_COMMIT_PINNED")

    def test_continue_on_error_true_is_rejected(self):
        violations = self.scan("jobs:\n  x:\n    continue-on-error: true\n")
        self.assertEqual(violations[0].code, "CONTINUE_ON_ERROR_TRUE")

    def test_flow_style_continue_on_error_is_rejected(self):
        violations = self.scan(
            "jobs: {x: {runs-on: ubuntu-24.04, continue-on-error: true}}\n"
        )
        self.assertTrue(
            any(v.code == "CONTINUE_ON_ERROR_TRUE" for v in violations),
            violations,
        )

    def test_flow_style_uses_is_rejected_fail_closed(self):
        sha = "a" * 40
        violations = self.scan(
            f"jobs: {{x: {{steps: [{{uses: actions/checkout@{sha}}}]}}}}\n"
        )
        self.assertTrue(
            any(v.code == "USES_SYNTAX_UNSUPPORTED" for v in violations),
            violations,
        )

    def test_quoted_uses_key_is_enforced(self):
        violations = self.scan('steps:\n  - "uses": actions/checkout@v4\n')
        self.assertEqual(violations[0].code, "POLICY_KEY_SYNTAX_UNSUPPORTED")

    def test_quoted_continue_on_error_key_is_enforced(self):
        violations = self.scan("jobs:\n  x:\n    'continue-on-error': true\n")
        self.assertEqual(violations[0].code, "POLICY_KEY_SYNTAX_UNSUPPORTED")

    def test_explicit_policy_key_is_rejected_fail_closed(self):
        violations = self.scan("? uses\n: actions/checkout@v4\n")
        self.assertEqual(violations[0].code, "POLICY_KEY_SYNTAX_UNSUPPORTED")

    def test_alias_mapping_key_is_rejected_fail_closed(self):
        violations = self.scan(
            "policy_key: &policy_key uses\nsteps:\n  - *policy_key: actions/checkout@v4\n"
        )
        self.assertEqual(violations[0].code, "POLICY_KEY_SYNTAX_UNSUPPORTED")

    def test_escaped_quoted_mapping_key_is_rejected_fail_closed(self):
        violations = self.scan('steps:\n  - "u\\u0073es": actions/checkout@v4\n')
        self.assertEqual(violations[0].code, "POLICY_KEY_SYNTAX_UNSUPPORTED")

    def test_shell_json_in_block_scalar_is_not_a_yaml_key(self):
        workflow = """steps:
  - run: |
      echo '{"schema":"omni.test.v1"}'
"""
        self.assertEqual(self.scan(workflow), [])

    def test_json_printf_inside_run_block_is_not_yaml_mapping(self):
        workflow = """jobs:
  hil:
    runs-on: ubuntu-24.04
    steps:
      - run: |
          printf '{"schema":"omni.hil-run-binding.v1","head_sha":"%s"}\\n' \
            "$GITHUB_SHA" > binding.json
"""
        self.assertEqual(self.scan(workflow), [])

    def test_mutable_pip_inside_run_block_remains_rejected(self):
        workflow = """steps:
  - run: |
      python -m pip install -r requirements.txt
"""
        violations = self.scan(workflow)
        self.assertTrue(
            any(v.code == "PIP_INSTALL_WITHOUT_HASHES" for v in violations)
        )

    def test_local_action_is_allowed(self):
        self.assertEqual(self.scan("steps:\n  - uses: ./local-action\n"), [])

    def test_docker_requires_digest(self):
        violations = self.scan("steps:\n  - uses: docker://alpine:latest\n")
        self.assertEqual(violations[0].code, "DOCKER_NOT_DIGEST_PINNED")
        digest = "b" * 64
        self.assertEqual(self.scan(f"steps:\n  - uses: docker://alpine@sha256:{digest}\n"), [])

    def test_mutable_latest_runner_is_rejected(self):
        violations = self.scan("jobs:\n  x:\n    runs-on: ubuntu-latest\n")
        self.assertEqual(violations[0].code, "MUTABLE_RUNNER_LABEL")

        violations = self.scan(
            "strategy:\n  matrix:\n    os: [ubuntu-latest, windows-latest]\n"
        )
        self.assertTrue(violations)
        self.assertTrue(all(v.code == "MUTABLE_RUNNER_LABEL" for v in violations))

    def test_pinned_runner_is_allowed(self):
        self.assertEqual(
            self.scan("jobs:\n  x:\n    runs-on: ubuntu-24.04\n"),
            [],
        )

    def test_mutable_pip_installs_are_rejected(self):
        for command in (
            "python -m pip install -e .",
            "python3 -m pip install -r requirements.txt",
            "python3 -m pip install --upgrade -r requirements.txt",
            "pip3 install -r requirements.txt",
            "pip3.13 install -r requirements.txt",
        ):
            with self.subTest(command=command):
                violations = self.scan(f"steps:\n  - run: {command}\n")
                self.assertTrue(
                    any(v.code == "PIP_INSTALL_WITHOUT_HASHES" for v in violations)
                )

    def test_split_mutable_pip_install_is_rejected(self):
        workflow = """steps:
  - run: |
      python -m pip install \\
        --upgrade \\
        -r requirements.txt
"""
        violations = self.scan(workflow)
        self.assertTrue(
            any(v.code == "PIP_INSTALL_WITHOUT_HASHES" for v in violations)
        )

    def test_hash_locked_multiline_pip_install_is_allowed(self):
        workflow = """steps:
  - run: |
      python -m pip install \\
        --only-binary=:all: \\
        --require-hashes \\
        -r requirements-build.lock
"""
        self.assertEqual(self.scan(workflow), [])

    def test_policy_keywords_in_comments_are_ignored(self):
        workflow = """# continue-on-error: true
# uses: actions/checkout@v4
jobs:
  x:
    runs-on: ubuntu-24.04
    steps:
      - run: echo ok
"""
        self.assertEqual(self.scan(workflow), [])


if __name__ == "__main__":
    unittest.main()
