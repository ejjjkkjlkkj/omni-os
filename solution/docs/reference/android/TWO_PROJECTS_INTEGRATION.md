# AccessibleQEMU + AccessibleAndroid ISO

This repository contains two coordinated but independently testable projects.

## Project 1: AccessibleQEMU

Primary host target: Windows 11 26H2 x64.

Responsibilities:

- fully keyboard-navigable graphical VM manager;
- Microsoft UI Automation exposure for NVDA, JAWS and Narrator;
- QEMU x86_64 process management;
- WHPX acceleration when available, TCG fallback;
- QMP control channel;
- serial-console recovery path;
- ISO and virtual-disk attachment;
- snapshots, reset, pause, stop and export operations;
- accessible status, progress and diagnostic surfaces.

A release fails if a critical operation is pointer-only or inaccessible to screen readers.

## Project 2: AccessibleAndroid ISO

Primary guest target: Android 17 / API 37 x86_64.

Responsibilities:

- BIOS and UEFI bootable ISO;
- installable VM operating system, not merely an emulator image;
- persistent userdata;
- Android-native GPT/AVB/dynamic-partition layout;
- TalkBack available from first boot;
- offline eSpeak NG TTS;
- keyboard-only setup and installer paths;
- braille path;
- optional authorized GMS/Play Store edition;
- accessibility boot-health validation and rollback policy.

## Integration contract

AccessibleQEMU is the reference host used to boot and validate AccessibleAndroid.

The end-to-end release path is:

```text
AccessibleQEMU.exe
    -> QEMU x86_64
    -> OVMF/UEFI
    -> AccessibleAndroid-17-x86_64.iso
    -> blank virtual disk
    -> accessible installation
    -> reboot from installed disk
    -> Android accessibility/runtime validation
```

A combined release is not considered valid unless all of the following are true:

1. AccessibleQEMU itself is navigable with keyboard and exposes correct UIA semantics on Windows 11 26H2.
2. AccessibleAndroid boots from the ISO in the reference VM.
3. The installer can be completed without sight.
4. The installed Android instance boots without the ISO.
5. TalkBack and offline TTS are operational.
6. `/data` survives at least one reboot.
7. QMP/serial control remains available as an independent recovery channel.

## Artifact boundary

AccessibleQEMU artifacts:

```text
AccessibleQEMU.exe
AccessibleQEMU-portable.zip
```

AccessibleAndroid artifacts:

```text
AccessibleAndroid-17-x86_64.iso
AccessibleAndroid-17-x86_64.qcow2
AccessibleAndroid-17-x86_64.vdi
AccessibleAndroid-17-x86_64.vmdk
SHA256SUMS
build-manifest.json
```

The two projects must remain independently buildable so that a QEMU GUI regression cannot invalidate the Android source tree and an Android BSP regression cannot block maintenance of the host manager.
