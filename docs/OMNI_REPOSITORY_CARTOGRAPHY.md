# OMNI Repository Cartography

## Purpose
This document is the current repository-level map for continuing autonomous work without losing scope, dependencies, evidence boundaries, security, accessibility, or release constraints.

## Authority order
1. Executable repository state and reproducible evidence.
2. Security and accessibility source contracts.
3. Requirements and architecture contracts.
4. Verification and regression evidence.
5. Release gate.
6. Planning/documentation.
Documentation never upgrades an unverified state.

## Repository planes
| Plane | Current surfaces | Required evidence |
|---|---|---|
| Agent runtime | .omni-agent/agent.py, tool_runner.py, tools.json, tool_policy.json, run.ps1 | executable cycle, policy enforcement, negative cases |
| Knowledge/contracts | .omni-agent/knowledge/* | schema validation, provenance, fail-closed coverage |
| Patch/change control | patch_engine.py, checkpoint/evidence mechanisms | bounded reversible change + checkpoint + post-change verification |
| Evidence | evidence_ledger.py, cycle_evidence.py, state/ | secret-scrubbed reproducible records |
| Security | SECURITY.md, security contracts, validators, threat-intel, source registry | implementation + automated tests + negative cases + provenance |
| Accessibility | accessibility taxonomies, source proof, requirements, screen-reader paths | keyboard + speech + braille + degraded/recovery evidence |
| Database | db/, migrations/, scripts/check_database_* | schema, integrity, provenance, coverage, accessibility |
| Knowledge/taxonomy | data/taxonomy/, data/threat-intel/ | source provenance, deterministic normalization, coverage-gap detection |
| Core software | src/, tests/ | unit/integration tests, compatibility, performance, reproducibility |
| CI/release | .github/workflows/, release gate, repository contract | reproducible commands, artifacts, exact revision |
| UEFI/firmware | UEFI-related source/docs and hardware boundary | software proof separately from physical HIL proof |
| Hardware boundary | docs/HARDWARE_ONLY_BOUNDARY.md and HIL obligations | physical evidence only; never inferred from software tests |

## Cross-layer lifecycle
discover -> map -> baseline -> checkpoint -> change -> verify -> security -> accessibility -> regression -> provenance -> release decision -> next task
Every new defect or feature must be placed on this lifecycle before modification.

## Evidence boundary
Software evidence can establish deterministic software behavior. It cannot establish physical keyboard scan behavior, real HDA codec/amplifier/speaker output, physical latency/jitter, TPM quote behavior, or OEM-specific physical behavior unless corresponding hardware-in-the-loop evidence exists.

## Accessibility is first-class
Accessibility is independently evidenced alongside security. Required planes include semantic UI, keyboard-only, speech, braille, trusted accessibility paths, secret-safe I/O, accessible security prompts, degraded mode, offline recovery, and diagnostics.

## Security source of truth
The integrated source registry records immutable repository/blob references used by the security/accessibility contracts. Imported source metadata is provenance only; it does not constitute implementation or test evidence.

## Agent continuation rule
The agent must continue from the highest-priority unresolved evidence gap. It must not declare release while any required dimension or required evidence state is UNKNOWN, BLOCKED, ENVIRONMENT, REGRESSED, MISSING, or PARTIAL.

## Claude continuation handoff
When another coding agent such as Claude continues this repository, it should first inspect this map, the engineering line, execution ladder, release gate, source registry, and current executable state. It should then reproduce the baseline before changing code, preserve existing behavior, and take the smallest evidence-backed next step.

## Current integration branch
`integration/omni-security-unification`
The exact commit history remains authoritative; this map is a navigation aid, not a substitute for executable evidence.