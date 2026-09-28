# Changelog

All notable repository changes are tracked here.

## Unreleased

### Added
- Voice frontend: deterministic dotted-numeric speech token so firmware version
  and decimal strings (e.g. `BIOS 1.2.3`, `3.5 volts`) are spoken as one
  `version` token joined by "point", never split into digits by sentence-ending
  clauses. Plain integers, acronyms and real end-of-sentence dots are unchanged.
- Exact-commit software-ceiling aggregation with fail-closed evidence handling.
- Deterministic UEFI, QEMU/OVMF, SCT, formal-proof, fuzzing, coverage, and reproducibility gates.
- Explicit hardware-only boundary and physical HIL workflow.
- SCT compatibility shim for EDK II stable 202608's non-versioned GCC toolchain profile.
- SCT compatibility shim for modern EDK II `Base.h`, backporting upstream CPU-marker detection for the pinned 202509 SCT.
- Deprecated-protocol compatibility backport for EDK II 202608, covering upstream build fix #300 and the required ENTS reference cleanup from #362.
- 0BSD licensing and public contribution metadata.

### Changed
- UEFI SCT build/runtime paths now target `RELEASE_GCC` instead of the removed `RELEASE_GCC5` profile.
- CI workflows use immutable action pins and locked external toolchain inputs.
- Pinned QEMU builds require and verify the libslirp user-network backend used by SCT runtime networking.
- Hardware-boundary aggregation now accepts exact-commit authorized `workflow_dispatch` HIL evidence while software-ceiling aggregation remains push-only.
- Final hardware-boundary enforcement now runs from completed Physical AMD HIL runs instead of taking a premature `main` push snapshot.

### Release status
The package version remains `0.1.0`. No tagged public release has been declared yet.
