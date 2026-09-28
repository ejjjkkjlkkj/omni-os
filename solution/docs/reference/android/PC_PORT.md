# Android 17 x86_64 PC/VM port

## Final product

The project must create a standalone Android operating system that boots from ISO and installs to a normal virtual disk. Android Studio and Cuttlefish are not required to run the released system.

## Upstream policy

Platform framework source tracks AOSP `android-latest-release` / `android17-release`.

PC support is forward-ported from maintained Android-on-PC work rather than downgrading the whole OS to an old Android-x86 release.

Primary implementation references:

- Android-x86 architecture and installer concepts: https://www.android-x86.org/
- Android Generic Project concepts: https://github.com/android-generic
- BlissRoms-x86 maintained PC trees: https://github.com/BlissRoms-x86
- current generic PC device reference: https://github.com/BlissRoms-x86/device_generic_common

## Components to forward-port

### Build/product

- `android_x86_64` style product registration;
- x86_64 BoardConfig;
- PC-specific init/fstab/uevent rules;
- system image packaging;
- `iso_img` build target.

### Boot

- UEFI x86_64 first;
- legacy BIOS compatibility second;
- GRUB/EFI or equivalent open boot loader;
- kernel + initrd live environment;
- live boot and install boot entries.

### Kernel

Start from Android 17 GKI 6.18 policy and add/configure the virtual hardware required by PC hypervisors:

- virtio-blk / virtio-scsi;
- virtio-net;
- virtio-input;
- virtio-gpu / DRM;
- virtio-rng;
- USB HID;
- generic SATA/AHCI fallback;
- common Intel HDA/AC97/virtio audio paths as needed;
- ext4/vfat and installer filesystem support.

### Graphics

Tier 1 is QEMU/KVM with virtio-gpu and Mesa. A software rendering fallback is mandatory so accessibility does not depend on working 3D acceleration.

### Networking

Tier 1 uses virtio-net. E1000-style fallback may be retained for hypervisors that do not expose virtio.

### Installation

Installer requirements:

1. enumerate disks without modifying them;
2. speak the selected disk, size and destructive action;
3. create GPT/EFI layout by default;
4. install the Android system payload;
5. create persistent userdata;
6. install bootloader;
7. verify the installed system before reboot;
8. allow a keyboard-only cancel/recovery route at every destructive step.

## Accessibility-specific boot contract

The installer and installed OS share the same accessibility policy:

- audio must initialize before interactive setup;
- offline TTS is bundled;
- screen reader is bundled and enabled by accessibility bootstrap;
- no network account is needed to obtain speech;
- keyboard navigation is complete;
- recovery can restore screen-reader/TTS defaults;
- braille services are part of the release qualification matrix.

## VM qualification

### QEMU/KVM

Canonical CI target. Must support unattended machine creation, ISO boot, install, reboot and adb-based validation.

### VirtualBox

Must boot/install with generic virtual hardware and software graphics fallback if accelerated graphics is unavailable.

### VMware

Must boot/install using generic virtual devices where possible. Hypervisor-specific integration is optional and cannot be required for basic accessibility.

### Hyper-V

Deferred until its device model and graphics/audio paths pass the same accessibility gates.

## Milestones

1. Android 17 x86_64 product compiles.
2. Kernel boots to Android init in QEMU.
3. Android reaches launcher from virtual disk.
4. ISO live boot works.
5. Installer writes a fresh VM disk and boots it.
6. Audio + offline TTS work.
7. Screen reader works on first boot.
8. Keyboard-only installation/setup passes.
9. QEMU automated end-to-end test passes.
10. VirtualBox/VMware compatibility pass.
11. Signed reproducible release artifacts are published.
