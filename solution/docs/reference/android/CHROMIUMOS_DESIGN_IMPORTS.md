# ChromiumOS design imports for Accessible Android VM

ChromiumOS is used only as an engineering reference for PC/VM robustness. Accessible Android remains an Android/AOSP operating system and does not embed ChromiumOS or ChromeOS Flex binaries.

## What we adopt

1. Declarative GPT disk layouts, with distinct installed and VM/removable layouts.
2. A/B boot semantics with explicit boot-success state and rollback after a failed update.
3. Separation between immutable system content and persistent user state.
4. EFI System Partition for standard x86_64 UEFI boot.
5. Recovery/update design that tolerates an interrupted update without destroying the working slot.
6. Reproducible image generation where partition numbers and labels remain stable over time.

## What we do not copy

- ChromeOS kernel/rootfs GUID types.
- ChromeOS firmware verified-boot protocol.
- ROOT-A/ROOT-B filesystem model.
- ChromeOS Flex binaries, firmware or proprietary Google payloads.
- ARC/ARCVM as the Android runtime: Android itself is the host OS in this project.

## Android-native mapping

Accessible Android implements the same resilience goals with Android-native mechanisms:

- `esp` — FAT32 EFI System Partition with the x86_64 bootloader.
- `boot_a`, `boot_b` — GKI kernel slots.
- `init_boot_a`, `init_boot_b` — generic ramdisk slots for modern Android.
- `vendor_boot_a`, `vendor_boot_b` — PC/VM vendor ramdisk and modules.
- `vbmeta_a`, `vbmeta_b` — Android Verified Boot metadata.
- `super` — one physical dynamic-partition container; Android logical slot partitions live inside it.
- `metadata` — update/snapshot and boot metadata.
- `misc` — Android boot/recovery control data.
- `userdata` — persistent encrypted user/application data, never stored in `super`.

A dedicated `recovery` partition is not required for the reference VM design; recovery resources should live in `vendor_boot` unless the BSP proves a dedicated recovery image is necessary.

## Boot/update policy

The bootloader must maintain slot priority, retry count and successful-boot state. An update is written to the inactive Android slot. The new slot becomes active only for the next boot attempt. Android userspace marks it successful only after the accessibility-critical boot milestone is reached: framework ready, audio available, TTS service available and screen reader service enabled. If that milestone is not reached within the boot-health window, the boot chain must return to the previous slot.

This project deliberately makes accessibility part of boot health: an update that boots visually but leaves a blind user without speech is considered a failed update.

## Image classes

The build system should eventually expose three layouts from one declarative partition specification:

- `installed`: full A/B layout for long-lived VM disks and physical-PC installation.
- `installer`: bootable ISO/USB image containing the installer/recovery environment and payload.
- `vm`: preinstalled sparse/raw/qcow2 disk image with the same installed GPT identities and expandable `userdata`.

The same partition manifest must drive ISO installation, raw/qcow2 generation and validation tests to prevent layout drift.
