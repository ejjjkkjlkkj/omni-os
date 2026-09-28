# solution — Omni Software Ceiling

Goal: push software verification until the remaining uncertainty is genuinely hardware-specific.

The repository is not a PsExec clone. It develops a cross-layer research stack for UEFI accessibility and deterministic validation: semantic firmware UI, read-only firmware modeling, tamper-evident event chains, model checking, fuzzing, replay, formal verification gates, reproducible builds and hardware-in-the-loop validation.

## Current bootstrap

Run directly from the source tree; no editable install is required.

```powershell
$env:PYTHONPATH = "src"
python -m omni.cli toolchain
python -m unittest discover -s tests -v
```

```bash
export PYTHONPATH=src
python -m omni.cli toolchain
python -m unittest discover -s tests -v
```

## Definition of done

`SOFTWARE_CEILING_PASS` is allowed only when every required software gate is `PASS`. `NOT_RUN`, `SKIP`, or missing external verification tools remain blockers. The final hardware-only gates are physical UEFI behavior, keyboard scan behavior, real HDA codec/amplifier/speaker output, physical latency/jitter, TPM quote, and OEM-specific behavior.

See `docs/ARCHITECTURE.md` and `SECURITY.md`.

## Verification boundary

The exact-commit software verdict includes deterministic IFR mutation fuzzing and CI workflow/toolchain linting. The virtual UEFI proof fixes a QEMU SMBIOS Type 1 UUID and requires OmniProbe to report the same UUID; physical ASUS identity remains a separate HIL obligation.

See `docs/HARDWARE_ONLY_BOUNDARY.md` for the strict separation between software evidence and physical-only evidence.

## License

This repository is licensed under the Zero-Clause BSD license (`0BSD`). It permits use, copying, modification, and distribution for any purpose without an attribution requirement. See `LICENSE`.


## Integrated security + accessibility architecture

This repository also contains the unified OMNI security/accessibility knowledge base and verification contracts. Cybersecurity and accessibility are co-equal: every requirement must define both properties. Coverage spans Hardware -> UEFI -> PreEnvironment -> Recovery -> Loader -> Kernel -> System -> OS, plus public, unindexed, authenticated, restricted and overlay network surfaces. Unknown technologies or trust layers fail closed as coverage gaps.

Integrated controls include machine-readable taxonomies, source provenance, threat-intelligence normalization, database migrations, fail-closed coverage validators, accessibility source proof, security contract validation, and database integrity/provenance tests.

See `docs/SECURITY_ACCESSIBILITY_ARCHITECTURE.md`, `docs/MASTER_COVERAGE_MAP.md`, `data/taxonomy/master-coverage.json`, `data/taxonomy/accessibility-full-stack.json`, and `requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv`.