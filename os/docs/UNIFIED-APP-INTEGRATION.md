# Unified application integration

## Product rule

Accessible Windows must not expose Android, Linux or Darwin/macOS applications as separate desktops that the user has to enter and leave.

Once an application is installed and admitted by its compatibility runtime, it should behave like an application of the host system:

- searchable in the global launcher;
- pinnable and launchable from the normal shell;
- represented by a normal host window in task switching;
- able to open user-approved files and URLs through host brokers;
- able to publish notifications through the host notification center;
- able to exchange clipboard data under host policy;
- able to use host audio and networking services;
- able to register file/URL handlers without bypassing user choice;
- able to expose a semantic accessibility tree to the native screen reader;
- able to participate in host focus, keyboard navigation and lifecycle management;
- uninstallable from one host application-management surface.

The runtime remains isolated underneath. The user should not need to know which runtime owns an application during ordinary use.

## Research basis

The design uses public architecture references and clean-room implementation rules:

- WSLg demonstrates that Linux GUI applications can appear in the host launcher/task switcher and share clipboard behavior while still executing inside a Linux subsystem.
  - https://learn.microsoft.com/windows/wsl/tutorials/gui-apps
  - https://github.com/microsoft/wslg
- XDG Desktop Portal demonstrates a brokered model for files, URI opening and other desktop resources for sandboxed applications.
  - https://docs.flatpak.org/en/latest/portal-api-reference.html
- Android ART executes DEX bytecode and Android application IPC is based heavily on Binder. Android sandboxing and runtime services remain guest responsibilities.
  - https://source.android.com/docs/core/runtime
  - https://source.android.com/docs/core/architecture/hidl/binder-ipc
- Darling demonstrates the shape of a userspace Darwin compatibility environment: Mach-O loading, Mach/POSIX/Darwin interfaces and reimplemented frameworks rather than a full macOS virtual machine.
  - https://github.com/darlinghq/darling

These are design references only. Accessible Windows does not copy proprietary Windows or macOS source code or proprietary framework binaries.

## Common host application model

Every installed application is represented by one host-owned application record regardless of origin.

Conceptually the record contains:

- stable host application ID;
- runtime origin: native, Android, Linux or Darwin;
- package/bundle identity supplied by the runtime;
- display name and localized metadata;
- icon resources converted into host-safe cached assets;
- executable/runtime launch descriptor;
- declared file handlers;
- declared URI/protocol handlers;
- requested host capabilities;
- accessibility support level;
- architecture requirement;
- installation source and signature/provenance data;
- runtime version against which the app was last validated.

The shell indexes this host record, not the guest filesystem directly.

## One common host broker

Foreign applications do not receive unrestricted access to host internals. They call a common broker layer.

Required broker surfaces:

1. **Launcher/app registry**
   - publishes installed apps to search, launcher and task switcher;
   - maintains stable host IDs across runtime upgrades.
2. **Window integration**
   - maps guest/runtime surfaces to ordinary host windows;
   - host compositor owns placement, focus, task switching and accessibility window identity.
3. **File portal**
   - user selects or approves host files/directories;
   - runtime receives only explicit handles/mappings;
   - guest path syntax never becomes a host security boundary.
4. **Open-with and URI intents**
   - apps may declare capabilities;
   - host owns default-app selection and user consent;
   - handlers cannot silently replace a host default.
5. **Clipboard**
   - typed clipboard bridge with policy and size limits;
   - text/images/files are translated through host-defined formats.
6. **Notifications**
   - guest notifications become host notifications;
   - host owns focus, privacy, quiet-hours and accessible announcement policy.
7. **Audio**
   - foreign apps render/capture through host audio sessions;
   - microphone access is permission mediated.
8. **Networking**
   - per-runtime virtual networking with host firewall/policy;
   - no implicit bypass of host VPN/firewall/account policy.
9. **Accessibility**
   - guest semantic nodes are translated into the native accessibility tree;
   - focus/events/actions retain stable host application identity;
   - visual-only foreign windows do not count as accessibility support.
10. **Lifecycle**
    - start, suspend, terminate, crash reporting and resource limits are host controlled.

## Android applications

### Execution model

Android executes locally inside a hardware-isolated lightweight VM on x64 hardware.

The target stack is:

- minimal Linux kernel needed by Android runtime;
- AOSP userspace/framework components selected for compatibility;
- ART for DEX/OAT execution;
- Binder inside the Android guest;
- virtio-style devices/brokers to host services;
- host compositor bridge for individual Android windows;
- Android accessibility bridge into the host semantic tree.

The Android VM is an implementation detail and does not present a separate phone desktop unless explicitly requested for diagnostics.

### Installation model

Initial supported inputs should include APK installation. App-store integration is a separate policy/licensing problem and must not be assumed.

After installation:

- package identity is imported into the common host app registry;
- Android activities that can be launched are mapped to host launch descriptors;
- Android intents/file associations are mapped conservatively into host handlers;
- notifications are relayed to the host;
- Android accessibility nodes/events are translated to native semantics.

### CPU architecture

On an x86-64 host:

- DEX/ART code can execute through the Android runtime independently of the app's Java/Kotlin source architecture;
- native x86-64 libraries run directly inside the Android VM;
- ARM64 native libraries require an explicitly provided binary-translation/native-bridge implementation;
- absence of that translator must produce a clear compatibility error rather than silently failing.

## Linux applications

### Execution model

Linux applications execute locally in one or more hardware-isolated Linux environments rather than requiring the host kernel to implement the complete Linux syscall ABI.

The model is intentionally similar in spirit to WSL2/WSLg integration:

- Linux kernel in a lightweight VM;
- selected distribution/userspace managed independently;
- Wayland-first graphical integration;
- X11 compatibility only behind an isolated bridge where needed;
- host window compositor integration;
- host clipboard, notifications, files and URL portals;
- PulseAudio/PipeWire-like guest-facing audio bridge mapped to host audio sessions;
- semantic accessibility bridge from AT-SPI/accessible toolkit data into the host tree.

The user may install packages using the distribution's normal package manager, but applications exported to the host are registered individually in the common host app catalogue.

### Distribution handling

A Linux distribution is a runtime source, not a separate desktop identity.

Multiple distributions may exist, but two apps with the same desktop name remain separate host app records because their package provenance/runtime instance differs.

### CPU architecture

- x86-64 Linux binaries run natively inside the x86-64 guest VM;
- ARM64 Linux binaries require explicit translation on an x86-64 host;
- translation is optional and cannot weaken isolation.

## Darwin/macOS applications

### Execution model

Darwin application support is a clean-room compatibility personality, not bundled macOS and not a macOS VM.

The long-term stack is:

- Mach-O loader;
- dynamic-loader compatibility;
- Mach IPC/userspace server;
- Darwin/POSIX syscall personality;
- libSystem-compatible surface where legally implementable;
- Objective-C runtime compatibility;
- progressively reimplemented Foundation/AppKit/CoreFoundation/CoreAudio-like APIs;
- graphics translation to host compositor/GPU APIs;
- host accessibility translation from the compatibility framework's semantic objects.

Darling and GNUstep are research references for how such compatibility layers can be structured, but dependency/licensing decisions are made component by component.

### Installation model

Possible supported package forms include application bundles and package/disk-image formats only where a clean-room/legal parser is available.

Installing an `.app` or compatible package should create a normal host application record. The application should then launch from the same shell/search/task switcher as native, Android and Linux apps.

### Limits

Compatibility depends on implemented APIs. Running arbitrary current macOS GUI software cannot be promised until the required frameworks, graphics APIs, entitlement behavior and system services are reimplemented.

On the project's x86-64 host baseline:

- x86-64 Mach-O is the first native machine-code target;
- ARM64 Mach-O requires a future instruction translator;
- proprietary Apple frameworks are not bundled merely to increase compatibility.

## Accessibility integration

This is a hard requirement for all three compatibility families.

An application runtime cannot be marked fully integrated until it provides a semantic bridge to the native screen reader.

The bridge must preserve at least:

- application/window identity;
- role/control type;
- accessible name and description;
- value/state;
- focus;
- selection;
- text content and text ranges where provided by the guest toolkit;
- caret/selection events where provided;
- actions such as invoke, toggle, expand/collapse and set-value;
- live-region/notification semantics;
- keyboard navigation relationships.

If a foreign application only exposes pixels and no semantic accessibility, it may run but must be reported as accessibility-incomplete; it cannot be used as evidence that the compatibility subsystem is accessibility-complete.

## Host-wide behavior after install

Example: an Android PDF reader is installed.

Expected behavior:

1. it appears in the normal application search;
2. the user can pin it;
3. the host can offer it in **Open with** for PDFs if the app declares a compatible intent;
4. choosing a PDF grants only that file through the file portal;
5. the Android VM starts automatically if needed;
6. only the app window appears, not a mandatory Android desktop;
7. Alt-Tab treats it like other host windows;
8. clipboard and notifications use host policy;
9. the screen reader consumes translated Android accessibility semantics;
10. closing the last Android window allows the runtime VM to suspend automatically.

The same host-visible behavior is the target for Linux and Darwin applications.

## Security rules

- installing an app does not grant host filesystem access;
- runtime guests cannot register privileged host handlers without policy approval;
- guest root is not host root;
- clipboard/file/URL/audio/microphone/camera/device access is brokered;
- application provenance and runtime origin remain inspectable;
- compatibility runtimes cannot modify the immutable host system generation;
- a compromised runtime must be containable without compromising the host kernel;
- compatibility updates are versioned independently and may be rolled back if integration/accessibility health regresses.

## Ten-year longevity

The common app registry and broker protocol are versioned host contracts.

During a supported major generation:

- stable app IDs remain resolvable;
- runtime-origin metadata remains readable;
- broker protocols use explicit major/minor versions;
- new capabilities are additive where possible;
- old registered apps survive runtime upgrades or fail with an explicit compatibility reason;
- handler/launcher/accessibility semantics must not silently change into unsafe behavior;
- migrations are tested using fixtures from old supported runtime/app registry versions.

## Implementation order

1. finish the common `aw-app-compat` admission contract;
2. define the persistent host app registry format;
3. define broker protocol/versioning and capability tokens;
4. implement host launcher registration and lifecycle broker;
5. implement Linux x86-64 VM + Wayland window bridge first because it exercises the generic VM/desktop path;
6. reuse that VM/broker substrate for Android/AOSP/ART;
7. implement Android package/activity/intent/accessibility translation;
8. implement Darwin x86-64 Mach-O loader and CLI compatibility first;
9. expand Darwin framework/GUI compatibility incrementally;
10. add ARM64 binary translation only after x86-64 paths are secure, measurable and accessible.

## Definition of compatibility success

An app being executable is not enough.

For an Android, Linux or Darwin application to be called **host-integrated**, automated and physical testing must demonstrate launcher registration, ordinary host window management, file/URL portal behavior, clipboard, notifications, audio/network policy and native screen-reader semantics without requiring the user to enter a separate guest desktop.
