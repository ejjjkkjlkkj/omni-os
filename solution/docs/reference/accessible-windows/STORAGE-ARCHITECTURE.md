# Accessible Windows storage architecture

Status: design baseline for the x86-64 bootstrap. This document defines the target disk layout; it does not yet activate a new installer or on-disk format.

## Decision

Accessible Windows will not copy the classic single writable OS-volume layout used by Windows, and it will not copy the complete ChromeOS partition table verbatim. The target is a small, PC-friendly GPT layout combining the strongest properties of Windows, ChromeOS, modern Android, and macOS:

- UEFI + GPT as the generic x86-64 boot baseline.
- Physical A/B boot and system slots for the first production design.
- Read-only, cryptographically authenticated system images.
- A separate encrypted mutable state volume.
- Automatic rollback after failed boots or failed health validation.
- A recovery environment independent of the active system slot and usable without sight.
- No security decision may trust GPT fields without bounds and consistency validation.

Physical A/B is deliberately preferred over snapshot/Virtual-A/B for the first implementation. It costs more storage but keeps the boot/update state machine small, auditable, and independent from a complex copy-on-write storage layer. A snapshot-based design can be evaluated later after the block layer, crash consistency, and recovery logic are mature.

## Proposed GPT layout

| Order | Name | Initial sizing policy | Runtime policy | Purpose |
|---|---|---:|---|---|
| 1 | `AW_ESP` | 512 MiB | FAT32, normally read-only after boot | UEFI loader only. No user data. |
| 2 | `AW_BOOTMETA` | 32 MiB | tiny redundant metadata records | Slot priority, tries remaining, successful-boot state, rollback index, checksums. |
| 3 | `AW_BOOT_A` | 256 MiB | read-only outside updater | Signed kernel/boot image for slot A. |
| 4 | `AW_SYSTEM_A` | image-sized | read-only + authenticated | Core OS, drivers required for boot, built-in accessibility stack and system applications for slot A. |
| 5 | `AW_BOOT_B` | 256 MiB | read-only outside updater | Signed kernel/boot image for slot B. |
| 6 | `AW_SYSTEM_B` | same capacity as A | read-only + authenticated | Inactive/rollback system image. |
| 7 | `AW_RECOVERY_A` | 1 GiB target | signed, read-only | Independent accessible recovery environment. |
| 8 | `AW_RECOVERY_B` | 1 GiB target | signed, read-only | Redundant recovery fallback. |
| 9 | `AW_STATE` | remaining usable space | encrypted, writable | Users, settings, logs, caches, app data, update staging and mutable machine state. |

Sizes are policy defaults, not ABI constants. The installer must calculate aligned sizes from actual media capacity and refuse layouts that cannot preserve both a known-good slot and recovery.

## System versus applications

System applications that are required for boot, accessibility, recovery, settings, networking, storage management, or security belong inside `AW_SYSTEM_A/B` and are updated atomically with the OS.

Third-party applications must not turn `AW_STATE` into a generic writable-and-executable filesystem. Application packages stored on mutable storage should be signed, content-addressed, verified before use, and exposed to the process loader through a verified execution path. Ordinary user data, downloads, caches, logs, temporary files, and application data should remain non-executable by default.

This keeps the security boundary close to the ChromeOS rule that executable system code comes from a verified read-only image while mutable state is kept elsewhere, while retaining the general-purpose application model expected on a PC.

## Slot state machine

`AW_BOOTMETA` is the source of mutable A/B state, not GPT attributes. It contains at least two independently checksummed records with monotonic sequence numbers. A record identifies the active/preferred slot, whether each slot is bootable, whether it has completed a successful boot, the remaining trial boots, and an anti-rollback version/index.

The boot loader must validate both metadata copies and choose the newest valid record. Corrupt, inconsistent, out-of-range, overlapping, or impossible partition metadata causes fail-closed recovery rather than speculative boot.

The normal update flow is:

1. Keep the current slot untouched and successful.
2. Write the inactive `AW_BOOT_*` and `AW_SYSTEM_*` images.
3. Verify signatures, complete image hashes/Merkle root, declared sizes, partition bounds, architecture, minimum bootloader version, and rollback policy.
4. Mark the target slot bootable, preferred, not-yet-successful, with a small retry count.
5. Reboot into the new slot.
6. Mark it successful only after kernel, storage, security services, update service, and the minimum accessibility/recovery path have passed health checks.
7. If trial boots are exhausted, automatically return to the previous successful slot.

The updater must never modify the currently running system slot.

## Integrity and confidentiality

`AW_SYSTEM_A/B` and both recovery images are immutable during normal operation. Their manifests are signed and their contents are authenticated block-by-block using a Merkle-tree/verity-style design. The trusted boot path validates the signed root measurement before treating a slot as executable.

`AW_STATE` is encrypted at rest. Mutable paths are non-executable by default. Device keys should use TPM 2.0 sealing when available, with a separately documented fallback for machines without a usable TPM. Anti-rollback should use TPM monotonic/NV state when available; software-only rollback protection must be explicitly treated as weaker.

The ESP is not a general storage area. Boot binaries and manifests loaded from it must be signature-verified. After the loader has obtained what it needs, the running OS should avoid mounting the ESP writable during normal use.

## GPT threat model

GPT is discovery metadata, not a root of trust. Every partition entry consumed before or during boot must be validated for:

- integer overflow and underflow;
- disk bounds;
- required alignment;
- overlaps and aliases;
- duplicate or unexpected critical GUIDs;
- slot pairing consistency;
- expected minimum/maximum sizes;
- image extent versus partition extent;
- consistency with the signed slot manifest.

A malicious GPT must not be able to redirect a verified manifest toward attacker-controlled sectors.

## Recovery and accessibility

Recovery is a first-class boot target, not an afterthought. Both recovery slots must use the same signature and integrity rules as the main OS. At minimum recovery must expose deterministic keyboard navigation, text/serial diagnostics, and the earliest practical speech path so a blind user can inspect boot/update failure, select a known-good slot, repair state, reinstall a signed image, or export diagnostics without requiring a graphical-only workflow.

A successful production boot should not be marked successful until the core accessibility service required by the product policy is operational. This prevents an update that technically reaches a desktop but leaves a blind user unable to operate the machine from being treated as a healthy release.

## Why this layout

Windows provides a clean UEFI/GPT baseline with a dedicated ESP and recovery partition, but its normal layout keeps the OS and most installed software on one writable Windows volume.

ChromeOS provides the strongest directly relevant precedent for physical A/B kernel/rootfs slots, a separate stateful partition, verified read-only root filesystems, automatic fallback, and independent recovery images.

Android demonstrates a mature slot state machine and the rule that the active slot is never modified by an OTA update. Virtual A/B reduces storage overhead but requires a substantially more complex snapshot/COW layer, so it is a later optimization rather than the bootstrap design.

macOS demonstrates the useful semantic split between a cryptographically sealed read-only System volume and a mutable Data volume, including the distinction between system applications and user-installed applications.

## Research references

- Microsoft Learn — UEFI/GPT-based hard drive partitions: https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/configure-uefigpt-based-hard-drive-partitions
- ChromiumOS — Disk format: https://www.chromium.org/chromium-os/developer-library/reference/device/disk-format/
- ChromiumOS — File System/Autoupdate: https://www.chromium.org/chromium-os/chromiumos-design-docs/filesystem-autoupdate/
- ChromiumOS — Verified Boot: https://www.chromium.org/chromium-os/chromiumos-design-docs/verified-boot/
- Android Open Source Project — A/B system updates: https://source.android.com/docs/core/ota/ab
- Android Open Source Project — OTA / Virtual A/B: https://source.android.com/docs/core/ota
- Apple Platform Security — Signed system volume security: https://support.apple.com/guide/security/signed-system-volume-security-secd698747c9/web
- Apple Platform Security — Role of Apple File System: https://support.apple.com/guide/security/seca6147599e/web
- UAPI Group — Discoverable Partitions Specification: https://uapi-group.org/specifications/specs/discoverable_partitions_specification/

## Implementation sequence

The storage implementation should proceed in this order: strict GPT parser and partition-bound validation; immutable slot manifest format; redundant boot metadata state machine; signed boot image verification; authenticated system-image reads; inactive-slot updater; successful-boot/rollback logic; encrypted state volume; accessible recovery; then, only after crash-consistency and fuzz testing are mature, evaluate snapshot-based Virtual A/B to recover storage capacity.
