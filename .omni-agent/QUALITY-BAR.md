# OMNI engineering quality bar

## Devise

**Faire mieux. Jamais moins.**

This is an engineering rule, not a claim that the system is perfect. Every change must improve a measurable property, close an evidence gap, or remove a verified defect without silently weakening another dimension.

## Non-negotiable contract

1. Local and zero-cost first.
2. No API key, subscription or cloud dependency.
3. Security and accessibility are co-equal, including screen-reader, keyboard, speech and braille operability.
4. Unknown stays unknown; missing, blocked, timeout and environment failures are not successes.
5. Writes are bounded by an explicit target, recoverable checkpoint and deterministic verification path.
6. Regression is first-class.
7. Evidence is reproducible: baseline identity, changed artifact identity, tool result and provenance.
8. Secrets never become agent state.
9. No autonomous destructive Git operations: no push, history rewrite, hard reset or recursive deletion.
10. Release readiness requires explicit evidence for required dimensions.

## Agent loop

discover -> map -> understand -> baseline -> checkpoint -> change -> targeted verify -> security -> accessibility -> regression -> evidence -> release gate -> next

## Failure taxonomy

- PASS: declared check ran and succeeded.
- FAIL: declared check ran and failed.
- UNKNOWN: execution could not establish a result, including timeout.
- BLOCKED: policy prevented execution.
- ENVIRONMENT: infrastructure/toolchain/harness prevented a meaningful code judgment.
- REGRESSION: previously proven behavior no longer passes.
- PROVEN: requirement has implementation plus requirement-specific evidence.
- RELEASED: required release gates are proven.

UNKNOWN and BLOCKED never become PASS automatically.

## External engineering alignment

SWE-bench evaluates repository-level issue resolution using real codebases and tests; its current issue tracker also exposes failure modes involving reproducibility, environment fidelity, output contamination and test tampering. OMNI therefore treats reproducibility and harness integrity as engineering requirements.

OpenSSF Scorecard describes continuous checks across repository security practices, CI testing, SAST and build-process risk. OMNI maps these ideas into explicit local evidence domains without requiring the Scorecard service itself.

## Definition of done

A task is done only when the requirement is identified; baseline and target are recorded; the change is bounded and recoverable; targeted verification passes; security and accessibility impact is tested where applicable; regression checks pass; provenance/evidence is recorded; and unresolved gaps remain visible.
