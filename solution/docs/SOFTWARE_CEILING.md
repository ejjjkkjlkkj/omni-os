# Software ceiling contract

A property is not allowed to remain a hardware question if it can be reproduced in an emulator, model checker, sanitizer, fuzzer, host test, static analyzer or independent build runner.

## Required software evidence

- semantic, IFR, SCT-summary and physical-evidence verifier tests
- strict native diagnostics plus GCC -fanalyzer and Clang static analysis
- ASan + UBSan
- 100% line/function/region/branch coverage for the bounded protocol core
- 100k+ coverage-guided fuzz smoke
- CBMC proof of MM and accessibility invariants
- Frama-C Eva + WP/RTE proof obligations
- GCC/Clang differential execution
- EDK II official host tests with ASan
- OmniProbe.efi built by pinned EDK II edk2-stable202608
- OVMF/QEMU TCG execution with fail-closed HII/evidence/final markers
- deterministic QEMU record/replay with blkreplay
- independent rebuild of OmniProbe.efi from source on a separate pinned Ubuntu 24.04 job
- independent execution of that rebuilt EFI under OVMF/QEMU with a commit-bound 256-bit challenge
- independent extraction and verification of OMNI-EVIDENCE.TXT from the mutated FAT image
- bit-identical native output from independent runners
- bit-identical OmniProbe.efi from independent runners
- deterministic source bundle built only from Git-tracked files
- embedded bundle manifest binding canonical path, byte length and SHA-256 for every tracked file
- provenance statement binding the bundle SHA-256 to the exact Git commit
- GitHub/Sigstore provenance attestation plus verification
- official UEFI SCT build and runtime against pinned OVMF
- SCT runtime OVMF built with QEMU host UEFI variables plus Secure Boot/authenticated-variable support
- SCT runtime storage exposed as NVMe with Q35 SATA disabled, so ATA/SCSI are outside that deterministic platform profile
- physical-media preparation + verifier bound to the expected EFI SHA-256 and a fresh 256-bit challenge

## Pinned supply-chain inputs

EDK II stable 202608 is pinned to commit 2970e5699ba6267f3384ffab20f96647578aebc8.
SCT runtime/build uses edk2-test-stable202509 commit 2b2a16ac239cd89d778cb79ae6e42c533fc4c25a with the same edk2-stable202608 commit 2970e5699ba6267f3384ffab20f96647578aebc8 used by the primary OVMF/OmniProbe proof path. The pinned SCT 202509 build script is deterministically adapted from its removed `GCC5` toolchain name to EDK II 202608's `GCC` profile; any unexpected upstream script drift fails closed.

GitHub artifact attestations are treated as SLSA v1.0 Build Level 2 evidence. This repository does not claim Build Level 3 solely from an attestation.

## Exact software/hardware boundary

`SOFTWARE_CEILING_PASS` is software-only. It MUST NOT require the Physical AMD HIL workflow.

The HIL workflow is evaluated separately. Once every software gate is PASS, any remaining uncertainty is allowed to be hardware/OEM-specific rather than an unclosed software question.

The independent verification gate is not allowed to pass from an OVMF launch alone. It must rebuild OmniProbe.efi from the pinned source/toolchain, boot that exact rebuild, validate the commit-bound challenge, reject every OMNI_*_FAIL marker, and extract the evidence file produced by that execution.

## SCT distinction

A successful SCT build or an SCT run against OVMF is not a conformance result for the ASUS firmware. OEM conformance exists only after the applicable SCT suites execute against that physical target and the result set is parsed under an explicit policy.

## Hardware-only boundary

Only after all applicable software gates are PASS may remaining uncertainty be assigned to:

- ASUS M1603QA physical OEM UEFI execution
- real pre-OS keyboard scanning/focus timing
- physical HDA codec, amplifier/EAPD and speaker acoustics
- real interrupt/timing/jitter behavior
- TPM hardware quote/measurement behavior
- other electrical/OEM-specific behavior

NOT_RUN, SKIP, timeout, missing marker, stale/mismatched physical challenge, missing artifact or unverified provenance must never become PASS.

## Immutable GitHub Actions and runners

Every external GitHub Action is pinned to a 40-hex commit SHA. Local actions are allowed; Docker actions must use a SHA-256 image digest.

Evidence workflows must use explicit runner generations such as `ubuntu-24.04` or `windows-2025`; mutable labels such as `ubuntu-latest`, `windows-latest`, and `macos-latest` are blocking policy violations.

The mandatory workflow-policy gate also rejects `continue-on-error: true`, preventing later edits from silently weakening the software ceiling.
## Additional mandatory gates

The software ceiling includes the deterministic `IFR Parser Fuzz` gate and the `CI Workflow Lint` gate. The latter validates workflow syntax, immutable action references, repository workflow policy, and the locked external verification toolchain.

The QEMU/OVMF proof uses a deterministic SMBIOS Type 1 UUID and requires OmniProbe to expose that identity in initial and record/replay execution. This is software evidence only. Matching the UUID of the real ASUS platform remains part of physical HIL and is not a prerequisite for `SOFTWARE_CEILING_PASS`.

The remaining physical-only uncertainty is defined in `docs/HARDWARE_ONLY_BOUNDARY.md`.
