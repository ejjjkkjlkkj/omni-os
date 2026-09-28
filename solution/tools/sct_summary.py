#!/usr/bin/env python3
from __future__ import annotations

import argparse
import codecs
from collections import Counter
import json
import pathlib
import re

FAIL_LINE = re.compile(r"(?:--\s*FAILURE\s*$|:\s*\[FAILED\]\s*$)", re.IGNORECASE)
ERROR_COUNT = re.compile(r"\b(?:Errors?|Failures?)\.*\s*[:=]?\s*([0-9]+)\s*$", re.IGNORECASE)
PASS_LINE = re.compile(r"--\s*PASS\s*$", re.IGNORECASE)
WARN_LINE = re.compile(r"--\s*WARNING\s*$", re.IGNORECASE)
BASELINE_SCHEMA = "omni.sct-baseline.v1"


def decode_summary_bytes(data: bytes) -> str:
    """Decode SCT output without silently corrupting result markers."""
    if data.startswith((codecs.BOM_UTF16_LE, codecs.BOM_UTF16_BE)):
        return data.decode("utf-16")
    if data.startswith(codecs.BOM_UTF8):
        return data.decode("utf-8-sig")

    sample = data[:4096]
    pairs = len(sample) // 2
    if pairs >= 8:
        even_nuls = sum(byte == 0 for byte in sample[0 : pairs * 2 : 2])
        odd_nuls = sum(byte == 0 for byte in sample[1 : pairs * 2 : 2])
        threshold = max(8, int(pairs * 0.30))
        if odd_nuls >= threshold and odd_nuls >= even_nuls * 4:
            return data.decode("utf-16-le")
        if even_nuls >= threshold and even_nuls >= odd_nuls * 4:
            return data.decode("utf-16-be")

    return data.decode("utf-8")


def parse(text: str) -> dict[str, object]:
    explicit_failures = []
    error_counts = []
    passes = 0
    warnings = 0
    for number, line in enumerate(text.splitlines(), 1):
        stripped = line.rstrip()
        if FAIL_LINE.search(stripped):
            explicit_failures.append({"line": number, "text": stripped[-500:]})
        match = ERROR_COUNT.search(stripped)
        if match and int(match.group(1)) > 0:
            error_counts.append(
                {"line": number, "count": int(match.group(1)), "text": stripped[-500:]}
            )
        if PASS_LINE.search(stripped):
            passes += 1
        if WARN_LINE.search(stripped):
            warnings += 1

    failures = len(explicit_failures) + sum(item["count"] for item in error_counts)
    positive_evidence = passes > 0
    return {
        "schema": "omni.sct-summary.v1",
        "passes_observed": passes,
        "warnings_observed": warnings,
        "explicit_failure_lines": explicit_failures,
        "nonzero_error_counts": error_counts,
        "failure_score": failures,
        "positive_evidence": positive_evidence,
        "status": "PASS" if failures == 0 and positive_evidence else "FAIL",
    }


def parse_path(path: pathlib.Path) -> dict[str, object]:
    return parse(decode_summary_bytes(path.read_bytes()))


def _counter_dict(counter: Counter[object]) -> dict[str, int]:
    return {str(key): counter[key] for key in sorted(counter, key=lambda value: str(value))}


def _failure_text_counts(result: dict[str, object]) -> dict[str, int]:
    return dict(
        sorted(
            Counter(
                str(item["text"])
                for item in result["explicit_failure_lines"]  # type: ignore[index]
            ).items()
        )
    )


def _error_value_counts(result: dict[str, object]) -> dict[str, int]:
    return _counter_dict(
        Counter(
            int(item["count"])
            for item in result["nonzero_error_counts"]  # type: ignore[index]
        )
    )


def _diff_counts(current: dict[str, int], expected: dict[str, int]) -> dict[str, int]:
    keys = set(current) | set(expected)
    return {
        key: current.get(key, 0) - expected.get(key, 0)
        for key in sorted(keys)
        if current.get(key, 0) != expected.get(key, 0)
    }


def compare_baseline(
    result: dict[str, object], baseline: dict[str, object]
) -> dict[str, object]:
    if baseline.get("schema") != BASELINE_SCHEMA:
        raise ValueError("unsupported or missing SCT baseline schema")

    expected_obj = baseline.get("expected")
    if not isinstance(expected_obj, dict):
        raise ValueError("SCT baseline expected object is missing")

    for name in ("passes_observed", "warnings_observed", "failure_score"):
        if not isinstance(expected_obj.get(name), int):
            raise ValueError(f"SCT baseline {name} must be an integer")

    expected_failures = expected_obj.get("explicit_failure_text_counts")
    expected_errors = expected_obj.get("nonzero_error_counts")
    if not isinstance(expected_failures, dict) or not all(
        isinstance(key, str) and isinstance(value, int)
        for key, value in expected_failures.items()
    ):
        raise ValueError("invalid SCT baseline explicit failure counts")
    if not isinstance(expected_errors, dict) or not all(
        isinstance(key, str) and isinstance(value, int)
        for key, value in expected_errors.items()
    ):
        raise ValueError("invalid SCT baseline nonzero error counts")

    observed_failures = _failure_text_counts(result)
    observed_errors = _error_value_counts(result)
    scalar_mismatches = {}
    for name in ("passes_observed", "warnings_observed", "failure_score"):
        current = result[name]
        expected = expected_obj[name]
        if current != expected:
            scalar_mismatches[name] = {"observed": current, "expected": expected}

    failure_drift = _diff_counts(observed_failures, expected_failures)
    error_drift = _diff_counts(observed_errors, expected_errors)
    match = not scalar_mismatches and not failure_drift and not error_drift

    return {
        "schema": BASELINE_SCHEMA,
        "match": match,
        "context": baseline.get("context", {}),
        "scalar_mismatches": scalar_mismatches,
        "failure_text_count_drift": failure_drift,
        "nonzero_error_count_drift": error_drift,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("summary")
    parser.add_argument("--json-out")
    parser.add_argument("--baseline")
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    result = parse_path(pathlib.Path(args.summary))
    strict_pass = result["status"] == "PASS"

    if args.baseline:
        baseline_path = pathlib.Path(args.baseline)
        baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
        raw_status = result["status"]
        baseline_report = compare_baseline(result, baseline)
        result["raw_status"] = raw_status
        result["baseline"] = baseline_report
        result["status"] = "PASS" if baseline_report["match"] else "FAIL"
        strict_pass = bool(baseline_report["match"])

    output = json.dumps(result, indent=2, ensure_ascii=False)
    print(output)
    if args.json_out:
        pathlib.Path(args.json_out).write_text(output + "\n", encoding="utf-8")
    return 1 if args.strict and not strict_pass else 0


if __name__ == "__main__":
    raise SystemExit(main())
