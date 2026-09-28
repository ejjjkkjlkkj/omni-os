# Accessible Virtual Suite

This branch integrates the existing AccessibleAndroid build/ISO work with a new Windows-native AccessibleUTM frontend.

## Source roles

- `main`: Android 17 build, installer, preinstalled disk, boot validation and current AccessibleQEMU work.
- `utm-source-reference`: unmodified UTM source reference imported from upstream.
- `accessible-virtual-suite`: integration branch. It must retain the Android build chain while progressively porting useful UTM/QEMU concepts to Windows.

## Current architecture

```text
AccessibleUTM Windows
  -> serialized VM configuration
  -> architecture/profile selection
  -> QEMU command backend
  -> QMP lifecycle control
  -> QEMU
      -> x86_64 through WHPX with TCG fallback on Windows
      -> ARM64 through TCG
      -> RISC-V 64 through TCG
  -> guest media
      -> AccessibleAndroid ISO
      -> preinstalled AccessibleAndroid disk
      -> generic Linux/Windows/other ISO and disks
```

## AccessibleAndroid is a first-class guest

The existing Android x86_64 contract is intentionally preserved. AccessibleUTM must not silently change these values:

- machine: `q35`
- primary Android disk ID: `osdisk`
- primary Android disk PCI address: `0000:00:06.0`
- virtio RNG PCI address: `0000:00:07.0`
- virtio network PCI address: `0000:00:08.0`
- deterministic preinstalled disk size: 16 GiB
- Android A/B partition layout and `super`, `metadata`, `misc`, `userdata` partitions remain owned by the Android disk builder

This pinning is required because Android first-stage init uses the boot-device contract defined in `config/vm.env`.

## Accessibility contract

The Windows frontend must be fully operable without a mouse. Every essential VM operation must have a keyboard-reachable control and a meaningful accessibility name/state. Validation targets are:

- NVDA
- JAWS
- Windows Narrator
- Windows UI Automation exposure
- keyboard-only VM creation, editing, start, pause, resume, reset, graceful shutdown and force stop

Process detection alone is not considered accessibility validation. Real UIA/screen-reader task tests remain required on the Windows 11 26H2 self-hosted machine.

## Porting strategy from UTM

Use UTM as an architectural and behavioral reference while keeping the Windows implementation portable:

1. configuration model
2. QEMU argument generation
3. VM lifecycle/state model
4. QMP manager
5. removable drives and media
6. snapshots
7. networking
8. audio
9. SPICE display/input/clipboard
10. USB redirection
11. shared directories
12. templates and VM library
13. import/export

Apple-only SwiftUI/AppKit/Virtualization.framework code is not a Windows dependency. Equivalent Windows/QEMU implementations are required instead.

## Near-term milestones

1. Build and test `accessible-utm` on GitHub Actions Windows.
2. Preserve the existing AccessibleAndroid x86_64 PCI command contract in unit and CI tests.
3. Finish the Android boot chain until the real framework reports `sys.boot_completed=1`.
4. Start that exact image from AccessibleUTM.
5. Add persistent VM configuration and a VM library.
6. Add SPICE display/audio/clipboard and accessible device management.
7. Add ARM64 guest profiles without regressing the x86_64 Android image.

A feature is not considered complete until its real guest/runtime validation passes.
