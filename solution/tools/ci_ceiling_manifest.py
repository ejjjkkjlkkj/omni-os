#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

SOFTWARE_WORKFLOWS: dict[str, tuple[str, tuple[str, ...]]] = {
    "Software Ceiling": (
        ".github/workflows/software-ceiling.yml",
        ("core_semantics", "physical_evidence_verifier"),
    ),
    "Native Verification": (
        ".github/workflows/native-verification.yml",
        ("native_verification",),
    ),
    "Deep Software Ceiling": (
        ".github/workflows/deep-software-ceiling.yml",
        ("deep_software_verification",),
    ),
    "Deep Software Gates": (
        ".github/workflows/deep-software-gates.yml",
        ("deep_regression_gates",),
    ),
    "Independent Software Ceiling Verification": (
        ".github/workflows/independent-verification.yml",
        ("independent_verification",),
    ),
    "Formal Semantic Proof": (
        ".github/workflows/formal-semantic-proof.yml",
        ("formal_semantic_proof",),
    ),
    "IFR Parser Fuzz": (
        ".github/workflows/ifr-fuzz.yml",
        ("ifr_parser_fuzz",),
    ),
    "CI Workflow Lint": (
        ".github/workflows/workflow-lint.yml",
        ("ci_workflow_lint",),
    ),
    "Reproducibility and Provenance": (
        ".github/workflows/reproducibility.yml",
        ("reproducibility_and_provenance",),
    ),
    "UEFI SCT Build": (
        ".github/workflows/uefi-sct-build.yml",
        ("uefi_sct_build",),
    ),
    "UEFI SCT Runtime": (
        ".github/workflows/uefi-sct-runtime.yml",
        ("uefi_sct_runtime_ovmf",),
    ),
}

HARDWARE_WORKFLOWS: dict[str, tuple[str, tuple[str, ...]]] = {
    "Physical AMD HIL": (
        ".github/workflows/hardware-hil.yml",
        ("windows_qemu_hil", "windows_vmware_hil"),
    ),
}

SOFTWARE_EVENTS = frozenset({"push"})
HARDWARE_EVENTS = frozenset({"push", "workflow_dispatch"})

# Compatibility exports used by tests and callers that only need gate mappings.
WORKFLOW_TO_GATES = {
    name: gates for name, (_path, gates) in SOFTWARE_WORKFLOWS.items()
}
WORKFLOW_PATHS = {
    name: path for name, (path, _gates) in SOFTWARE_WORKFLOWS.items()
}
HARDWARE_WORKFLOW_TO_GATES = {
    name: gates for name, (_path, gates) in HARDWARE_WORKFLOWS.items()
}
HARDWARE_WORKFLOW_PATHS = {
    name: path for name, (path, _gates) in HARDWARE_WORKFLOWS.items()
}


def _run_order(run: dict[str, Any]) -> tuple[int, str, int]:
    return (
        int(run.get("run_number") or 0),
        str(run.get("created_at") or ""),
        int(run.get("id") or 0),
    )


def _latest_run(
    runs: list[dict[str, Any]],
    name: str,
    expected_path: str,
    head_sha: str,
    allowed_events: frozenset[str],
) -> dict[str, Any] | None:
    candidates = [
        run
        for run in runs
        if run.get("name") == name
        and run.get("path") == expected_path
        and run.get("head_sha") == head_sha
        and run.get("event") in allowed_events
    ]
    if not candidates:
        return None

    # The newest canonical run from an explicitly accepted event is
    # authoritative. Older cancelled duplicates naturally lose to a newer run,
    # while a newer cancellation must remain visible and block PASS instead of
    # reviving stale successful evidence.
    return max(candidates, key=_run_order)


def build_for_mapping(
    runs_payload: dict[str, Any],
    head_sha: str,
    workflows: dict[str, tuple[str, tuple[str, ...]]],
    *,
    allowed_events: frozenset[str] = SOFTWARE_EVENTS,
) -> tuple[dict[str, str], dict[str, Any]]:
    raw_runs = runs_payload.get("workflow_runs")
    runs = (
        [run for run in raw_runs if isinstance(run, dict)]
        if isinstance(raw_runs, list)
        else []
    )
    manifest: dict[str, str] = {}
    details: dict[str, Any] = {
        "head_sha": head_sha,
        "accepted_events": sorted(allowed_events),
        "workflows": {},
        "ready": True,
    }

    for workflow, (expected_path, gates) in workflows.items():
        run = _latest_run(
            runs,
            workflow,
            expected_path,
            head_sha,
            allowed_events,
        )
        if run is None:
            status = "MISSING"
            details["ready"] = False
            details["workflows"][workflow] = {
                "expected_path": expected_path,
                "status": "missing",
                "conclusion": None,
            }
        else:
            run_status = str(run.get("status") or "unknown")
            conclusion = run.get("conclusion")
            if run_status != "completed":
                status = "NOT_RUN"
                details["ready"] = False
            elif conclusion == "success":
                status = "PASS"
            else:
                status = str(conclusion or "FAIL").upper()

            details["workflows"][workflow] = {
                "expected_path": expected_path,
                "path": run.get("path"),
                "id": run.get("id"),
                "run_number": run.get("run_number"),
                "event": run.get("event"),
                "status": run_status,
                "conclusion": conclusion,
                "url": run.get("html_url"),
            }

        for gate in gates:
            manifest[gate] = status

    return manifest, details


def build(
    runs_payload: dict[str, Any],
    head_sha: str,
) -> tuple[dict[str, str], dict[str, Any]]:
    return build_for_mapping(runs_payload, head_sha, SOFTWARE_WORKFLOWS)


def build_hardware(
    runs_payload: dict[str, Any],
    head_sha: str,
) -> tuple[dict[str, str], dict[str, Any]]:
    return build_for_mapping(
        runs_payload,
        head_sha,
        HARDWARE_WORKFLOWS,
        allowed_events=HARDWARE_EVENTS,
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("runs_json", type=Path)
    ap.add_argument("--head-sha", required=True)
    ap.add_argument("--manifest-out", type=Path, required=True)
    ap.add_argument("--details-out", type=Path, required=True)
    ns = ap.parse_args()

    payload = json.loads(ns.runs_json.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise SystemExit("workflow runs payload must be a JSON object")

    manifest, details = build(payload, ns.head_sha)
    ns.manifest_out.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    ns.details_out.write_text(
        json.dumps(details, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {"manifest": manifest, "details": details},
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
