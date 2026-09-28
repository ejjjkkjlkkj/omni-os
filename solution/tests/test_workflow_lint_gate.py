import unittest

from omni.ceiling import REQUIRED_GATES
from tools.ci_ceiling_manifest import WORKFLOW_PATHS, WORKFLOW_TO_GATES


class WorkflowLintAndFuzzGateTests(unittest.TestCase):
    def test_ifr_fuzz_is_required_and_path_bound(self):
        self.assertIn("ifr_parser_fuzz", REQUIRED_GATES)
        self.assertEqual(WORKFLOW_TO_GATES["IFR Parser Fuzz"], ("ifr_parser_fuzz",))
        self.assertEqual(WORKFLOW_PATHS["IFR Parser Fuzz"], ".github/workflows/ifr-fuzz.yml")

    def test_workflow_lint_is_required_and_path_bound(self):
        self.assertIn("ci_workflow_lint", REQUIRED_GATES)
        self.assertEqual(WORKFLOW_TO_GATES["CI Workflow Lint"], ("ci_workflow_lint",))
        self.assertEqual(WORKFLOW_PATHS["CI Workflow Lint"], ".github/workflows/workflow-lint.yml")


if __name__ == "__main__":
    unittest.main()
