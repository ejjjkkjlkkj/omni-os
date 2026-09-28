# ChromeOS Flex reference analysis

## Decision

ChromeOS Flex is **not** a base operating system for Accessible Android.

Google's own ChromeOS Flex documentation states that Android applications and the Google Play Store are not supported on ChromeOS Flex. Therefore using the Flex image as the base would conflict with the project's goal: a complete Android system with Android application compatibility and an optional licensed Google Play edition.

The current Google recovery image URL supplied for analysis is:

```text
https://dl.google.com/chromeos-flex/images/latest.bin.zip
```

This binary is used only as an external engineering reference. It must never be copied, vendored, repackaged, or redistributed by this repository.

## What is useful to study

ChromeOS/ChromiumOS remains a strong reference for PC/VM boot engineering:

- x86_64 boot on legacy BIOS and UEFI;
- GPT disk layout;
- EFI System Partition;
- persistent state partition;
- A/B kernel and root filesystem concepts;
- boot-attempt/success metadata and rollback concepts;
- recovery layout;
- VM-oriented disk layouts;
- robust installation from removable media to fixed storage.

These architectural ideas can improve Accessible Android without importing ChromeOS code or binaries into Android.

## What must not be inherited

Do not use ChromeOS Flex as the Android runtime and do not make the Android system depend on:

- ChromeOS Flex proprietary binaries;
- ChromeOS Flex firmware payloads;
- ChromeOS-specific verified-boot kernel partition formats;
- ChromeOS account/session stack;
- ChromeOS update payloads;
- unsupported hacks intended to add Android/Play Store to Flex.

## Accessible Android adaptation

For Accessible Android, retain Android-native partitions and AVB while applying the useful robustness concepts:

```text
GPT
├── EFI        FAT32, UEFI boot files
├── boot_a     Android boot image A
├── boot_b     Android boot image B
├── vendor_boot_a / vendor_boot_b when required
├── super      dynamic system/vendor/product partitions
├── metadata   Android metadata
├── userdata   persistent /data
└── recovery / installer support as required
```

The exact layout must follow Android 17 build requirements and the selected PC BSP. The ChromeOS A/B model is a design reference, not a partition layout to copy literally.

## VM target

The final image must boot independently in a normal PC virtual machine:

- QEMU/KVM: reference implementation;
- VMware: compatibility target;
- VirtualBox: compatibility target;
- Hyper-V: compatibility target after Gen2/UEFI validation.

Output remains Android-native:

```text
AccessibleAndroid-17-x86_64.iso
AccessibleAndroid-17-x86_64.qcow2
AccessibleAndroid-17-x86_64.vdi
AccessibleAndroid-17-x86_64.vmdk
```

## Accessibility requirement

None of the ChromeOS boot ideas may delay accessibility. Audio, offline TTS, screen reader startup and keyboard navigation remain boot-critical release gates.
