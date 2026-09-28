import unittest
from pathlib import Path

from tools.workflow_policy import inspect

WORKFLOWS = Path(__file__).resolve().parents[1] / ".github" / "workflows"


class WorkflowPolicyRepoTests(unittest.TestCase):
    def test_actual_workflows_satisfy_the_policy(self):
        # The unit tests only scanned synthetic fixtures, so real policy
        # violations in the committed workflows slipped through. This scans the
        # actual .github/workflows tree the CI lint gate enforces (--strict).
        violations = inspect(WORKFLOWS)
        self.assertEqual(
            violations,
            [],
            "\n".join(f"{v.path}:{v.line} {v.code} {v.value}" for v in violations),
        )


if __name__ == "__main__":
    unittest.main()
