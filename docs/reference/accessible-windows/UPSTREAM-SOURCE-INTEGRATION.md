# Upstream source integration policy

## Goal

Accessible Windows should reuse real upstream open-source operating-system code where doing so improves compatibility, security, maintenance or time-to-working-system, while keeping the host kernel and its security boundaries independently controlled.

The project therefore distinguishes between:

- **host code**: the Accessible Windows bootloader, Rust kernel, storage, recovery, shell, accessibility and host brokers;
- **guest/runtime code**: upstream Linux or Android code built into isolated compatibility runtimes;
- **compatibility code**: clean-room or separately licensed implementations of foreign userspace APIs such as Darwin/Mach/POSIX and Objective-C frameworks;
- **proprietary code**: not imported merely to improve compatibility.

This is an engineering policy, not legal advice. Every imported component still requires file/project-level license review before release.

## Linux source

Linux code may be used directly as the kernel of an isolated Linux compatibility runtime.

Preferred model:

1. keep the Accessible Windows host kernel independent;
2. fetch a pinned, verified Linux source revision into the build pipeline;
3. build a minimal x86-64 guest kernel and userspace for the Linux application runtime;
4. expose only virtio/broker interfaces needed for graphics, input, files, clipboard, notifications, audio, networking and accessibility;
5. register individual Linux applications in the common host app catalogue rather than exposing a mandatory guest desktop;
6. publish the corresponding source and project modifications as required by the applicable Linux licenses when distributing guest binaries.

The Linux kernel upstream COPYING file identifies the kernel as GPL-2.0 with the Linux syscall note, with additional licenses applying to some files. This means Linux source is useful to us, but it should remain an auditable guest component rather than being copied piecemeal into the Rust host kernel.

### Initial Linux components to evaluate

- upstream Linux kernel, x86-64;
- a minimal init/userspace;
- Wayland-facing guest compositor bridge;
- PipeWire/PulseAudio-compatible guest audio endpoint;
- AT-SPI accessibility export;
- XDG desktop metadata and portal-compatible resource requests;
- optional Flatpak-compatible package/runtime support later.

## Android source

AOSP source may be used directly to construct the Android compatibility runtime.

AOSP states that Apache 2.0 is its preferred license and that the majority of Android userspace software uses Apache 2.0, while the Linux kernel portions remain under GPLv2 and individual projects may use other approved licenses. Therefore license metadata must be preserved per component.

Preferred Android model:

1. obtain Android from the official AOSP source/manifest rather than an opaque prebuilt image;
2. pin the exact platform revision used for every Accessible Windows release;
3. build the Android guest from source;
4. use a minimal Linux kernel appropriate to the Android runtime;
5. retain ART, Binder and the AOSP framework pieces required for application compatibility;
6. bridge graphics, input, clipboard, notifications, files, audio, networking and accessibility to host-owned services;
7. export installed Android applications individually into the common host app catalogue;
8. keep Google Mobile Services, Play Store and other proprietary Google packages out of the base runtime unless separately licensed and explicitly enabled.

### Initial Android components to evaluate

- ART / DEX runtime;
- Binder IPC;
- package manager and activity manager services;
- SurfaceFlinger/graphics integration pieces where appropriate;
- Android accessibility services and semantic node export;
- AOSP input/audio/network services needed by applications;
- native-bridge interface for future ARM64-to-x86-64 translation.

The Android runtime remains a hardware-isolated guest even though its applications appear as ordinary host applications.

## Darwin and macOS source

Darwin is not the same thing as macOS.

Apple publishes XNU and other Darwin components under open-source licenses such as APSL 2.0. XNU itself combines Mach, BSD and IOKit-related code and is available publicly. Those sources may be studied and, where the exact license permits and the architecture benefits, used as separately tracked upstream components.

However, macOS as a complete product contains proprietary frameworks, applications, assets and services that are not made open source merely because XNU is open source. Accessible Windows therefore must not treat the XNU repository as permission to bundle macOS.

Preferred Darwin compatibility model:

1. keep the Accessible Windows host kernel independent rather than replacing it with XNU;
2. implement a Darwin userspace personality capable of loading Mach-O applications;
3. use public Darwin interfaces and open-source Darwin components only after component-level license review;
4. reuse or learn from open-source projects such as Darling, GNUstep and libobjc2 where their licenses and architecture fit;
5. reimplement missing Foundation/AppKit/CoreFoundation/CoreAudio-like behavior incrementally through clean-room/public-interface work;
6. translate resulting windows, input, audio, files, notifications and accessibility semantics into host-native services;
7. never require a macOS installation or macOS VM for ordinary application compatibility;
8. never import proprietary Apple frameworks or binaries simply to increase compatibility.

### XNU role

XNU is valuable as:

- a public reference for Mach/Darwin kernel interfaces;
- a source of open components where use is license-compatible;
- a conformance reference for Mach messages, syscalls, process semantics and other public Darwin behavior.

XNU is **not** currently planned as the Accessible Windows host kernel and is not required to run Darwin applications if the userspace compatibility personality implements the required ABI and APIs.

## Repository layout rule

Third-party operating-system source must remain visibly separated from original host code.

Target structure:

```text
crates/                     # original Accessible Windows host contracts and services
boot/                       # original host boot code
kernel/                     # original Rust host kernel
runtime-manifests/          # pinned upstream source manifests and hashes
runtime-build/              # reproducible recipes for guest runtimes
third_party/                # only vendored components intentionally tracked in-tree
licenses/third-party/       # notices and exact upstream license texts required for distribution
```

Large upstream trees should normally be fetched reproducibly from pinned revisions rather than copied into the main Git history. Vendoring is reserved for components where reproducibility, review or upstream availability justifies it.

## Mandatory provenance for every upstream component

No source is admitted merely because it is on GitHub or publicly downloadable.

Every upstream component must record:

- canonical upstream project;
- exact source URL;
- exact commit/tag/revision;
- cryptographic source hash where practical;
- detected license(s);
- local modifications/patch set;
- build recipe and toolchain version;
- produced artifact hashes;
- update/rollback compatibility;
- security owner and update channel;
- accessibility impact.

A component without known provenance or known licensing fails closed and is excluded from release images.

## Accessibility requirement

Using upstream Linux, Android or Darwin source does not bypass the project's accessibility rules.

A runtime is not considered integrated until its applications expose meaningful semantics through the host accessibility bridge. In particular:

- Linux must bridge AT-SPI/toolkit semantics;
- Android must bridge Android accessibility nodes/events;
- Darwin compatibility frameworks must expose semantic objects that can be translated to the host tree;
- pixel-only output never counts as accessibility completeness;
- speech/braille and keyboard-only workflows must be tested on real hardware.

## Security boundary

Foreign source code is treated as potentially compromiseable.

Linux and Android execute behind hardware-assisted VM boundaries by default. Darwin compatibility executes behind a constrained userspace compatibility boundary with explicit host brokers. Foreign runtimes cannot write the immutable host generation or directly access host kernel internals.

The common brokers remain the only supported path for host files, URI handlers, clipboard, notifications, audio, microphone/camera, networking and accessibility integration.

## First implementation order

1. add a machine-readable pinned upstream source manifest format;
2. add reproducible Linux x86-64 guest-kernel build support;
3. build a minimal Linux runtime and prove one individually integrated accessible GUI application;
4. add an official-AOSP source manifest and reproducible Android guest build;
5. prove APK install -> host catalogue -> host window -> accessibility bridge;
6. create a Darwin open-source component inventory with per-component licenses;
7. build the Mach-O loader and Darwin CLI compatibility surface before GUI frameworks;
8. evaluate selected Darling/GNUstep/libobjc2 components only after license and dependency review;
9. add automated source provenance/SBOM/license checks to CI;
10. maintain real application compatibility and accessibility corpora for Linux, Android and Darwin.

## Current decision

**Linux:** integrate real upstream source as an isolated guest runtime.

**Android:** integrate real AOSP source as an isolated guest runtime.

**Darwin/macOS:** integrate only open Darwin components that pass license review; implement the missing macOS application environment through clean-room/open-source compatibility layers rather than bundling macOS.

This preserves the product goal: applications from all three ecosystems appear as first-class Accessible Windows applications while the underlying runtime technology remains isolated and mostly invisible to the user.
