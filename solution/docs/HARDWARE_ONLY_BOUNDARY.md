# Hardware-only boundary

The repository may report `SOFTWARE_CEILING_PASS` only when every required software workflow for the exact commit has completed successfully.

Missing, pending, skipped, cancelled, timed-out, stale, neutral, action-required, or failed software evidence is not PASS. The final aggregator binds workflow name, canonical workflow path, event type, and exact commit SHA before accepting evidence.

After the software ceiling closes, remaining uncertainty is limited to phenomena that intrinsically require the physical target:

- execution inside the ASUS OEM UEFI;
- physical pre-OS keyboard scan/focus timing;
- the real HDA codec topology;
- amplifier/EAPD routing and the physical speaker path;
- audible speech quality on the real speaker path;
- physical interrupt/latency/jitter behavior;
- TPM quotes and measurements from the actual machine;
- OEM/electrical implementation behavior.

QEMU/OVMF, VMware compatibility, parsers, HII/IFR semantics, model checking, sanitizers, fuzzing, static analysis, formal proofs, UEFI SCT on OVMF, record/replay, workflow policy, toolchain pinning, reproducibility, provenance, and evidence verification remain software obligations and must not be relabeled as hardware limitations.

Physical HIL is tracked separately from `SOFTWARE_CEILING_PASS`. A software pass therefore does not claim that the physical machine, codec, speaker, keyboard, TPM, or OEM firmware has been validated.

The hardware manifest accepts only canonical `Physical AMD HIL` runs bound to the exact commit. Automatic `push` runs on `main` and authorized `workflow_dispatch` runs are valid hardware evidence sources; pull-request runs are rejected. The enforced hardware-status workflow is evaluated after the privileged HIL run completes, so an early snapshot cannot masquerade as a final hardware verdict.
