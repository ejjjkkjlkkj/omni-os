# Recovery and boot architecture

## Goal

Accessible Windows recovery must remain usable when the normal OS, the newest generation, boot metadata, networking, graphics, speech, or one storage copy fails. A blind user must be able to understand the failure and operate rollback/recovery without relying on a pointer or visual inspection.

This design is intentionally stricter than a conventional boot menu. Recovery is a first-class, independently validated system path.

## Research basis

The design takes concepts from public documentation without copying proprietary source code or on-disk formats:

- Microsoft Windows RE: automatic entry after repeated boot failures, separate recovery environment, local repair tools, boot-to-recovery control, and newer cloud remediation. Microsoft also documents that corruption of Boot Manager, BCD, or related disk metadata can make the normal on-disk WinRE path inaccessible.
  - https://learn.microsoft.com/windows-hardware/manufacture/desktop/windows-recovery-environment--windows-re--technical-reference
  - https://learn.microsoft.com/windows/configuration/quick-machine-recovery/
  - https://learn.microsoft.com/windows-hardware/manufacture/desktop/windows-re-troubleshooting-features
- GNU GRUB: keep persistent boot state deliberately small; `grubenv` is a preallocated block, `next_entry` supports one-shot boot, and `fallback` can select another entry.
  - https://www.gnu.org/software/grub/manual/grub/html_node/Environment-block.html
  - https://www.gnu.org/software/grub/manual/grub/html_node/fallback.html
- UAPI Boot Loader Specification and systemd Automatic Boot Assessment: per-entry attempt counting, explicit good/indeterminate/bad boot state, and automatic fallback after attempts are consumed.
  - https://uapi-group.org/specifications/specs/boot_loader_specification/
  - https://systemd.io/AUTOMATIC_BOOT_ASSESSMENT/
- OSTree: atomic transitions between complete bootable deployments so power loss leaves either the old deployment or the new deployment, never a half-updated system.
  - https://ostreedev.github.io/ostree/atomic-upgrades/
- ChromiumOS design: a minimal verified recovery path separated from normal writable firmware/software, signed recovery media, A/B fallback concepts, and manual recovery entry.
  - https://www.chromium.org/chromium-os/chromiumos-design-docs/firmware-boot-and-recovery/
  - https://www.chromium.org/chromium-os/chromiumos-design-docs/verified-boot/

These are architecture references only. Accessible Windows keeps its own clean-room implementation and formats.

## Core model

Accessible Windows uses four distinct boot states:

1. **Known-good generation** — last generation that passed the complete runtime health gate, including accessibility and accessible recovery.
2. **Trial generation** — new generation with a finite attempt budget. It is never promoted merely because the kernel or graphical shell appeared.
3. **Recovery Core** — small signed local recovery environment with a deliberately narrow dependency set.
4. **External recovery** — signed removable-media recovery image used if local boot/recovery metadata cannot be trusted or read.

An optional network remediation layer may exist above Recovery Core, but local recovery must not require network access.

## Better-than-conventional recovery rules

### 1. Do not make recovery depend on the same metadata that just failed

A normal OS entry and the local Recovery Core must have independent discovery paths.

The boot design must eventually include:

- primary signed boot application;
- independent signed Recovery Core entry;
- removable-media recovery path;
- standard UEFI x64 fallback path where practical;
- redundant, checksummed boot-state records outside mutable OS configuration;
- no requirement that a user repair a text configuration database before recovery can start.

Corrupt normal boot metadata must therefore not automatically imply "no recovery".

### 2. Bounded trial boot with explicit promotion

A new generation starts as `Trial { tries_remaining: N }`.

Before each attempt, the boot state is durably changed so sudden power loss cannot create an unlimited retry loop. Promotion to known-good occurs only after all required runtime health checks pass.

Required promotion signals include at minimum:

- kernel/runtime core;
- storage;
- input;
- audio;
- accessibility broker;
- speech;
- accessible recovery;
- security/update health.

If the attempt budget reaches zero, selection returns to the previous known-good generation.

### 3. One-shot boot selection

The system should support a one-shot boot request equivalent in intent to Windows Boot Manager `/bootsequence` or GRUB `next_entry`, but stored in the project's own redundant boot-state record.

Properties:

- applies to exactly the next boot;
- cleared/consumed atomically before transfer of control;
- cannot silently become a permanent default;
- target generation/recovery entry must still pass signature, anti-rollback and structural checks;
- one-shot state cannot weaken Secure Boot/verified-boot policy.

### 4. Atomic complete-system updates

Updates stage a complete immutable generation before changing boot selection.

The system must never update the live system tree in place for core OS components. Power loss at any persistence boundary must yield one of:

- old known-good generation;
- fully staged new trial generation;
- Recovery Core.

There must be no state in which half of one generation is combined with half of another for executable system content.

### 5. Tiny boot-writable state

Early boot code should write as little as possible.

Do not use frequently rewritten UEFI NVRAM variables for per-boot attempt counters. Use fixed-size redundant disk records with:

- magic/version;
- sequence number;
- selected generation;
- previous known-good generation;
- rollback floor;
- trial attempts;
- one-shot request;
- integrity checksum/authentication metadata;
- copy selection rules that reject equal-sequence conflicts instead of guessing.

The amount of mutable early-boot state must remain bounded and independently fuzzable.

### 6. Recovery Core is not a graphical rescue desktop

Recovery Core is considered usable only after the accessibility contract passes.

Before presenting a critical choice it must establish:

- deterministic keyboard input;
- structured diagnostics;
- speech and/or braille direct output;
- rollback selection;
- signed reinstall path;
- diagnostic export.

Visual rendering may mirror the same semantic model but cannot satisfy accessibility readiness.

### 7. One semantic event for every frontend

Each recovery failure/action is represented by one typed `RecoveryEvent` with a stable diagnostic code, severity, action and generation identity.

The identical event is sent to:

- structured diagnostics;
- speech;
- braille;
- optional visual frontend;
- optional serial/technician frontend.

A delivery proof is bound to that exact event identity. A successful speech/braille delivery for one event cannot be reused as evidence that another action was announced.

### 8. Deterministic keyboard recovery

Recovery navigation has a stable, documented order. Critical actions are never pointer-only.

Rules:

- no destructive action is selected by timeout;
- timeout means no action;
- signed reinstall and power-off require explicit confirmation;
- rollback to a known-good generation is distinct from reinstall;
- focus changes are announced nonvisually;
- action labels and diagnostic codes remain stable across compatible releases;
- the same keyboard sequence must not change from safe to destructive after an update.

### 9. Network remediation is optional and signed

A WinRE-like cloud remediation path is useful, but it is an enhancement rather than a dependency.

Recovery Core may:

1. inspect local diagnostics;
2. attempt local rollback first where safe;
3. optionally obtain a signed remediation manifest over authenticated transport;
4. verify signature, target hardware/OS generation constraints and anti-rollback policy locally;
5. apply remediation to a staged generation, never directly patch the known-good executable tree.

Loss of networking must not remove rollback, diagnostic export or signed removable-media reinstall.

### 10. External recovery remains possible when local recovery is damaged

External recovery media must be independently signed and verified before execution.

It must be capable of:

- reading/exporting diagnostics;
- validating local boot-state copies;
- selecting a still-valid known-good generation;
- reinstalling a signed system image;
- preserving user state when policy allows;
- refusing a destructive reinstall until target disk identity is announced and explicitly confirmed.

### 11. Ten-year longevity requirement

Every stable recovery/boot format is designed for a support horizon of at least ten years for a major generation.

Consequences:

- on-disk structures carry explicit major/minor versions and lengths;
- parsers ignore only explicitly defined forward-compatible fields and fail closed on incompatible major versions;
- diagnostic codes and safe keyboard semantics are append-only within a supported major generation;
- upgrade tooling must preserve at least one bootable rollback path across format migrations;
- migration tests cover old supported boot-state/manifest versions;
- recovery media for a supported generation must remain able to diagnose and reinstall that generation even after newer generations exist;
- cryptographic algorithm transitions require overlap periods and signed migration policy rather than a flag day;
- hardware support policy prefers stable standard interfaces and isolates vendor quirks behind versioned drivers.

## Recovery priority order

The default non-destructive decision order is:

1. validate boot-state copies and selected generation;
2. consume the current trial attempt before boot;
3. try selected generation if still eligible;
4. if trial is exhausted or structurally invalid, select previous known-good generation;
5. if normal generations cannot boot, enter local Recovery Core;
6. Recovery Core announces the exact failure nonvisually and offers rollback/diagnostic export;
7. optional signed network remediation may stage a repaired generation;
8. signed external recovery media remains the independent final recovery path;
9. destructive reinstall is never automatic.

## Failure policy

The implementation must fail closed for trust but fail usable for the human.

Examples:

- signature failure: do not execute the image; announce diagnostic and offer another verified generation/recovery path;
- speech failure with braille working: continue via braille and record speech failure;
- speech and braille unavailable: do not claim accessible recovery PASS;
- visual renderer failure: nonvisual recovery may continue;
- network failure: local rollback/export/reinstall media remain available;
- boot-state copy conflict: do not guess; enter recovery with a stable diagnostic;
- interrupted update: boot old known-good or fully staged trial, never partially updated content;
- corrupted local recovery: use independent signed external recovery path.

## Required automated tests

Before release-grade claims, CI/fault-injection must cover:

- trial attempt decrement before transfer;
- power loss at every boot-state write boundary;
- power loss at every generation staging/commit boundary;
- corrupted primary and secondary boot-state copies;
- equal-sequence conflicting copies;
- corrupted selected generation with healthy previous known-good;
- corrupted local Recovery Core with valid external recovery media;
- one-shot boot request consumption;
- no destructive timeout behavior;
- exact-event speech/braille delivery evidence;
- speech failure with braille fallback;
- both direct nonvisual channels absent -> fail closed;
- network unavailable during remediation;
- malicious/unsigned remediation rejection;
- old supported on-disk versions migrated across the ten-year compatibility window.

## Required physical blind-user validation

Automation cannot replace the human validation gate.

A blind user must be able to perform, from power-on without visual assistance:

- enter recovery manually;
- identify the currently selected generation and failure reason;
- boot previous known-good generation;
- export diagnostics;
- distinguish rollback from reinstall;
- cancel a destructive action safely;
- confirm a signed reinstall only after target-disk identity is spoken/brailled;
- recover when network is absent;
- recover when normal boot metadata is damaged;
- understand completion/failure state after every reboot.

AMD and Intel x64 physical machines are both mandatory before release-grade x64 recovery support is claimed.
