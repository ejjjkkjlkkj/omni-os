# Architecture

## Product objective

Accessible Android VM is an Android 17 x86_64 operating system that boots from ISO and installs to a VM disk. It must behave like an Android device from the guest point of view while remaining usable without sight from power-on through first-run setup and normal operation.

## Base platform

- AOSP manifest: `android-latest-release`
- Current platform: Android 17 / API 37
- Kernel family: Android 17 GKI 6.18 plus PC/virtualization drivers
- CPU architecture: x86_64
- Development variant: `userdebug`
- Release variant: hardened `user`

## Why this is not Cuttlefish

Cuttlefish is valuable for AOSP framework validation but it is a host-managed virtual device, not the final installable PC-style OS required by this project. The release artifact here must boot from standard VM firmware and install onto a normal virtual block device.

## PC BSP layer

AOSP alone does not provide the complete PC ISO/install stack. This project therefore maintains a PC BSP layer inspired by Android-x86 and Android-Generic concepts and forward-ports only the pieces needed for Android 17:

- x86_64 kernel configuration and VM drivers;
- virtio block/network/input/GPU/audio where practical;
- VMware/VirtualBox-compatible fallback devices;
- Mesa/DRM graphics path with software-rendering fallback;
- ALSA/audio policy for VM hardware;
- init and fstab rules for PC-style disks;
- EFI/BIOS boot chain;
- live-boot ramdisk;
- installer and persistent data partition handling;
- ISO packaging.

Old Android-x86 Android releases are reference implementations, not the platform base.

## Release formats

The canonical output is:

1. `AccessibleAndroid-17-x86_64.iso` — live/install media.
2. `AccessibleAndroid-17-x86_64.qcow2` — preinstalled QEMU/KVM disk.
3. `AccessibleAndroid-17-x86_64.vdi` — converted VirtualBox disk.
4. `AccessibleAndroid-17-x86_64.vmdk` — converted VMware disk.

All formats must derive from the same versioned build and publish checksums plus a build manifest.

## Disk layout

The installer should use GPT by default and create a simple recoverable layout:

- EFI System Partition when booting UEFI;
- Android system/root partition or immutable system image payload;
- persistent userdata partition;
- optional recovery/metadata area when required by the final update design.

Destructive actions must require explicit confirmation and must be spoken by the accessibility layer.

## Accessibility boot path

Accessibility is part of system bring-up, not an app-store dependency:

1. audio driver initializes;
2. offline TTS service becomes available;
3. accessibility bootstrap enables the bundled screen reader;
4. first-run UI exposes keyboard and screen-reader navigation;
5. installer and recovery expose spoken state and keyboard controls;
6. braille services initialize when a supported device is attached.

A release that boots visually but cannot speak is considered failed.

## Hypervisor matrix

### Tier 1

- QEMU/KVM with virtio devices.

### Tier 2

- VirtualBox x86_64.
- VMware Workstation/Fusion x86_64 where host architecture permits.

### Tier 3

- Hyper-V Gen2 after required synthetic-device compatibility is proven.

The ISO should prefer generic virtual hardware and retain software fallbacks rather than depending on one hypervisor.

## Android compatibility

The Android framework, PackageManager, ART, Binder, permissions, storage model and APK runtime remain normal Android. GMS/Google Play certification is a separate licensing/certification track and is not assumed by this open AOSP distribution.

## Release gates

Every release candidate must validate at minimum:

- ISO BIOS/UEFI boot;
- installation to an empty virtual disk;
- reboot from installed disk without ISO;
- persistent userdata across reboot;
- audio output;
- offline TTS synthesis;
- screen-reader enabled state;
- keyboard-only first boot;
- APK install/launch;
- networking;
- graphics with accelerated and software paths;
- accessibility-event smoke tests;
- braille path smoke test;
- QEMU/KVM boot test;
- VirtualBox and VMware compatibility tests when runners are available;
- Android CTS/VTS subsets applicable to the PC target.

## Definition of done

The project is done only when a blind user can create a fresh VM, attach the ISO, boot, hear usable speech, install Android to the virtual disk, reboot, complete setup and operate Android without requiring sight or a second inaccessible setup environment.