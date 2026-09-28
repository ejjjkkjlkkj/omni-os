# Changelog

All notable repository changes are tracked here.

## Unreleased

### Added
- Accessibility taxonomy source-of-truth, deterministically derived from
  committed authoritative data by `scripts/build_accessibility_taxonomy.py`:
  `full-stack-layers.json` (23 depth layers L16..L-6), `accessibility-full-stack.json`
  (21 publication layers + 23 stack layers, each exposing the full core-invariant
  capability set), and the official/overlay/stack accessibility source-proof files
  plus `actor-spectrum.json`. These required inputs were previously absent, so the
  entire accessibility and master-coverage validation chain could not run; it now
  passes end to end. Nothing is fabricated — every entry traces to `sources.json`,
  `network-layers.json`, and the accessibility/threat docs.
- Security contract: static `data/schema/security-knowledge.schema.json` JSON
  Schema describing the generated intelligence cache; the fail-closed validator
  required it but the file was absent, so the security gate could never pass.
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
- Accessibility source-proof validation now fails closed with an explicit
  `MISSING`/`INVALID` diagnostic and non-zero exit when a required accessibility
  taxonomy input is absent, empty, or malformed, instead of raising an uncaught
  traceback (shared `scripts/contract_io.require_json`). A missing input stays a
  blocking failure; it is never treated as pass.
- Security contract validator treats an empty generated intelligence cache the
  same as an absent one (updater CI populates it), instead of failing on the
  empty committed placeholder — keeping the gate fail-closed while offline.
- UEFI SCT build/runtime paths now target `RELEASE_GCC` instead of the removed `RELEASE_GCC5` profile.
- CI workflows use immutable action pins and locked external toolchain inputs.
- Pinned QEMU builds require and verify the libslirp user-network backend used by SCT runtime networking.
- Hardware-boundary aggregation now accepts exact-commit authorized `workflow_dispatch` HIL evidence while software-ceiling aggregation remains push-only.
- Final hardware-boundary enforcement now runs from completed Physical AMD HIL runs instead of taking a premature `main` push snapshot.

### Release status
The package version remains `0.1.0`. No tagged public release has been declared yet.
