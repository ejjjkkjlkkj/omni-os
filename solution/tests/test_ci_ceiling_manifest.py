import unittest

from omni.ceiling import HARDWARE_GATES, REQUIRED_GATES, evaluate, evaluate_hardware
from tools.ci_ceiling_manifest import (
    HARDWARE_WORKFLOW_PATHS,
    HARDWARE_WORKFLOW_TO_GATES,
    WORKFLOW_PATHS,
    WORKFLOW_TO_GATES,
    build,
    build_hardware,
)


class CiCeilingManifestTests(unittest.TestCase):
    SHA = "a" * 40

    def make_run(
        self,
        name,
        *,
        run_number,
        status="completed",
        conclusion="success",
        event="push",
        path=None,
    ):
        if path is None:
            path = WORKFLOW_PATHS.get(name) or HARDWARE_WORKFLOW_PATHS.get(name)
        return {
            "id": run_number,
            "name": name,
            "path": path,
            "head_sha": self.SHA,
            "event": event,
            "run_number": run_number,
            "created_at": f"2026-09-23T13:{run_number % 60:02d}:00Z",
            "status": status,
            "conclusion": conclusion,
            "html_url": f"https://example.invalid/{run_number}",
        }

    def test_complete_success_set_passes_exact_software_policy(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(set(manifest), set(REQUIRED_GATES))
        self.assertEqual(evaluate(manifest)["status"], "SOFTWARE_CEILING_PASS")

    def test_physical_hil_is_not_required_for_software_ceiling(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertNotIn("windows_qemu_hil", manifest)
        self.assertNotIn("windows_vmware_hil", manifest)
        self.assertEqual(evaluate(manifest)["status"], "SOFTWARE_CEILING_PASS")

    def test_hardware_manifest_is_separate(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(HARDWARE_WORKFLOW_TO_GATES)
        ]
        manifest, details = build_hardware({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertTrue(set(manifest).issubset(HARDWARE_GATES))
        complete = {gate: "PASS" for gate in HARDWARE_GATES}
        self.assertEqual(
            evaluate_hardware(complete)["status"],
            "HARDWARE_BOUNDARY_PASS",
        )

    def test_hardware_manifest_accepts_explicit_dispatch(self):
        runs = [
            self.make_run(
                name,
                run_number=i + 1,
                event="workflow_dispatch",
            )
            for i, name in enumerate(HARDWARE_WORKFLOW_TO_GATES)
        ]
        manifest, details = build_hardware({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(
            details["accepted_events"],
            ["push", "workflow_dispatch"],
        )
        self.assertTrue(
            all(status == "PASS" for status in manifest.values())
        )
        target = next(iter(HARDWARE_WORKFLOW_TO_GATES))
        self.assertEqual(
            details["workflows"][target]["event"],
            "workflow_dispatch",
        )

    def test_hardware_manifest_rejects_pull_request_evidence(self):
        runs = [
            self.make_run(
                name,
                run_number=i + 1,
                event="pull_request",
            )
            for i, name in enumerate(HARDWARE_WORKFLOW_TO_GATES)
        ]
        manifest, details = build_hardware({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])
        self.assertTrue(
            all(status == "MISSING" for status in manifest.values())
        )

    def test_software_manifest_rejects_workflow_dispatch_evidence(self):
        names = list(WORKFLOW_TO_GATES)
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(names)
        ]
        target = names[0]
        runs = [run for run in runs if run["name"] != target]
        runs.append(
            self.make_run(
                target,
                run_number=100,
                event="workflow_dispatch",
            )
        )
        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])
        for gate in WORKFLOW_TO_GATES[target]:
            self.assertEqual(manifest[gate], "MISSING")

    def test_missing_or_in_progress_is_not_ready(self):
        names = list(WORKFLOW_TO_GATES)
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(names[:-1])
        ]
        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])
        self.assertNotEqual(
            evaluate(manifest)["status"],
            "SOFTWARE_CEILING_PASS",
        )

        runs.append(
            self.make_run(
                names[-1],
                run_number=99,
                status="in_progress",
                conclusion=None,
            )
        )
        _manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])

    def test_failure_is_terminal_but_blocks_pass(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        runs[-1]["conclusion"] = "failure"
        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(evaluate(manifest)["status"], "SOFTWARE_INCOMPLETE")

    def test_pull_request_run_cannot_substitute_for_push_evidence(self):
        names = list(WORKFLOW_TO_GATES)
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(names)
        ]
        target = names[0]
        runs = [run for run in runs if run["name"] != target]
        runs.append(
            self.make_run(
                target,
                run_number=100,
                event="pull_request",
            )
        )
        _manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])

    def test_homonymous_workflow_wrong_path_cannot_substitute(self):
        names = list(WORKFLOW_TO_GATES)
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(names)
        ]
        target = names[0]
        runs = [run for run in runs if run["name"] != target]
        runs.append(
            self.make_run(
                target,
                run_number=500,
                path=".github/workflows/attacker-lookalike.yml",
            )
        )

        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])
        self.assertEqual(details["workflows"][target]["status"], "missing")
        for gate in WORKFLOW_TO_GATES[target]:
            self.assertEqual(manifest[gate], "MISSING")

    def test_wrong_path_newer_run_does_not_override_canonical_run(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        target = next(iter(WORKFLOW_TO_GATES))
        runs.append(
            self.make_run(
                target,
                run_number=999,
                conclusion="failure",
                path=".github/workflows/lookalike.yml",
            )
        )

        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(details["workflows"][target]["path"], WORKFLOW_PATHS[target])
        self.assertEqual(evaluate(manifest)["status"], "SOFTWARE_CEILING_PASS")

    def test_latest_canonical_push_run_controls_gate(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        target = next(iter(WORKFLOW_TO_GATES))
        runs.append(
            self.make_run(
                target,
                run_number=200,
                conclusion="failure",
            )
        )
        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(details["workflows"][target]["run_number"], 200)
        self.assertNotEqual(
            evaluate(manifest)["status"],
            "SOFTWARE_CEILING_PASS",
        )

    def test_newer_cancelled_run_blocks_older_same_sha_success(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        target = next(iter(WORKFLOW_TO_GATES))
        runs.append(
            self.make_run(
                target,
                run_number=500,
                status="completed",
                conclusion="cancelled",
            )
        )

        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(details["workflows"][target]["run_number"], 500)
        self.assertEqual(manifest[WORKFLOW_TO_GATES[target][0]], "CANCELLED")
        self.assertNotEqual(
            evaluate(manifest)["status"],
            "SOFTWARE_CEILING_PASS",
        )

    def test_older_cancelled_run_does_not_erase_newer_same_sha_success(self):
        target = next(iter(WORKFLOW_TO_GATES))
        runs = [
            self.make_run(
                target,
                run_number=1,
                status="completed",
                conclusion="cancelled",
            )
        ]
        runs.extend(
            self.make_run(name, run_number=i + 100)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        )

        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertTrue(details["ready"])
        self.assertEqual(details["workflows"][target]["run_number"], 100)
        self.assertEqual(evaluate(manifest)["status"], "SOFTWARE_CEILING_PASS")

    def test_in_progress_duplicate_still_blocks_older_success(self):
        runs = [
            self.make_run(name, run_number=i + 1)
            for i, name in enumerate(WORKFLOW_TO_GATES)
        ]
        target = next(iter(WORKFLOW_TO_GATES))
        runs.append(
            self.make_run(
                target,
                run_number=501,
                status="in_progress",
                conclusion=None,
            )
        )

        manifest, details = build({"workflow_runs": runs}, self.SHA)
        self.assertFalse(details["ready"])
        self.assertEqual(details["workflows"][target]["run_number"], 501)
        self.assertNotEqual(
            evaluate(manifest)["status"],
            "SOFTWARE_CEILING_PASS",
        )

    def test_malformed_runs_payload_fails_closed(self):
        manifest, details = build({"workflow_runs": "not-a-list"}, self.SHA)
        self.assertFalse(details["ready"])
        self.assertTrue(all(status == "MISSING" for status in manifest.values()))


if __name__ == "__main__":
    unittest.main()
