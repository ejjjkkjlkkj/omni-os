# Linux and Android guest VM architecture

## Terminology

`aw-vmm-plan` and `aw-x86-vmm-plan` are host virtual-memory/page-table planning crates. They are not a guest virtual-machine monitor.

Linux and Android compatibility use a separate guest-VM subsystem represented by `aw-guest-vm`. This distinction is intentional so host paging security and foreign-runtime virtualization cannot be confused or coupled accidentally.

## Security boundary

Linux and Android release runtimes must execute in a hardware-isolated virtual machine. A container or namespace arrangement that shares the Accessible Windows host kernel cannot satisfy the release gate.

The guest VM contract requires:

- hardware virtualization;
- separate guest memory;
- virtual CPUs and interrupt controller;
- verified guest image identity;
- crash containment;
- virtual block, network and input devices;
- entropy and monotonic clock sources;
- an explicit host IPC transport;
- a semantic accessibility bridge.

Graphical guests additionally require:

- virtual graphics;
- individual host window/surface integration;
- clipboard broker;
- notification broker;
- file portal;
- URL/intent broker;
- audio broker;
- host network-policy bridge.

A full guest desktop may exist for diagnostics but is not the normal application surface. Individual guest applications are exported through the common host application model.

## Candidate VM implementation

The initial external VMM candidate is crosvm, pinned in `upstreams/cross-runtime.lock.toml`. The pin is a research/build baseline, not evidence that crosvm is already integrated.

Before a VMM implementation can be admitted it must demonstrate at least:

1. VMX or AMD-V/SVM hardware execution on supported x86-64 hardware;
2. isolated guest physical memory;
3. bounded, audited device emulation;
4. host-controlled virtual block and network devices;
5. guest-to-host IPC that cannot access arbitrary host kernel objects;
6. crash/termination containment;
7. deterministic suspend/resume semantics where supported;
8. verified guest image loading;
9. compatibility with host update/rollback generations;
10. no bypass around host accessibility and permission brokers.

The implementation may evolve away from crosvm if measurements or licensing/integration constraints justify another VMM. The `aw-guest-vm` contract remains the stable host requirement.

## Linux guest

The Linux runtime contains a real Linux kernel and complete userspace source closure. Its normal GUI path is Wayland-first. Xwayland is present for applications that still require X11.

Guest integration services translate:

- Wayland surfaces to ordinary host windows;
- XDG portal operations to host file/URI permission brokers;
- clipboard formats to the host clipboard service;
- Linux notifications to the host notification center;
- PipeWire/audio streams to host audio sessions;
- guest networking to host network/firewall policy;
- AT-SPI semantics to the native accessibility tree.

Root inside the Linux guest is never host root.

## Android guest

The Android runtime contains the AOSP-derived userspace and required Linux-kernel contract. ART, Binder and Android framework services remain inside the guest.

Guest integration services translate:

- Android activities/surfaces to ordinary host windows;
- package/activity identity to the common host app registry;
- Android intents to host file/URI handlers;
- clipboard and notifications to host services;
- media/audio to host audio sessions;
- Android accessibility nodes/events/actions to native semantic accessibility;
- runtime permissions to the host broker policy where host resources are involved.

Google Play and proprietary Google packages are not implied by the open-source Android runtime.

## ARM64 applications on an x86-64 host

The host OS remains x86-64. ARM64 compatibility is an application/runtime translation feature, not an ARM host-kernel target.

The initial translation baseline is QEMU TCG, with family-specific adapters required for:

- Android Native Bridge use by ARM64 native libraries;
- Linux ARM64 application execution;
- future Darwin ARM64 Mach-O execution.

Translation must occur within the relevant compatibility isolation boundary and cannot grant direct access to host resources.

## Release proof chain

For Linux or Android to be considered release-grade, the proof chain is:

1. complete immutable runtime source closure from `aw-runtime-sources`;
2. verified guest image built from that closure;
3. successful `aw-guest-vm` admission;
4. successful `aw-app-compat` desktop integration admission;
5. real application conformance tests;
6. semantic screen-reader tests;
7. physical blind-user validation;
8. crash, rollback and update fault-injection tests.

A source manifest alone, a VM that merely boots, or an application that merely renders pixels is not sufficient.
