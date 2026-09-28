# Runtime source closure policy

## Meaning of "all"

For Accessible Windows compatibility runtimes, **all** means every source component required to build, reproduce, audit and legally redistribute every binary that ships as part of the Linux, Android or Darwin compatibility runtime.

It does not mean mirroring every program ever written for Linux, Android or macOS. User-installed applications remain separate packages. It does mean that no binary included in a runtime image may be an unexplained blob or an untracked dependency.

A runtime is not release-complete unless the repository can answer, for every shipped runtime binary:

1. where the source came from;
2. the exact immutable source revision;
3. the applicable license and notices;
4. the build recipe and toolchain inputs;
5. the runtime and build dependency closure;
6. the resulting binary identity/hash;
7. the SBOM/provenance record;
8. the accessibility path used by applications in that runtime.

## Host separation

The Accessible Windows host kernel remains the independent Rust kernel. Linux, Android and Darwin source code must not be copied into the host kernel merely to claim compatibility.

- Linux and Android execute behind the hardware-isolated VM boundary.
- Darwin/macOS application compatibility executes as a userspace compatibility personality.
- Host files, windows, clipboard, notifications, audio, networking and accessibility remain brokered by host-owned interfaces.

This keeps foreign runtime code replaceable and prevents a runtime compromise from automatically becoming host-kernel compromise.

## Android: full AOSP manifest closure

Android is sourced through the complete AOSP Repo manifest, not through a hand-curated subset of repositories.

The current baseline is recorded in `upstreams/android-aosp.lock.toml` and pins the Android 17 security manifest. A real source sync must:

1. initialize from the pinned platform manifest;
2. fetch every project selected by that manifest;
3. emit a resolved Repo manifest containing the exact commit for every project;
4. retain license/NOTICE metadata;
5. reject any runtime prebuilt that is not represented in provenance;
6. build the Android guest from that resolved closure;
7. record the resulting image hashes and SBOM.

Google Play, Play Services and other proprietary Google packages are not implicitly part of AOSP and cannot be silently added to satisfy compatibility.

## Linux: full installed-package source closure

The Linux compatibility runtime is a real Linux environment. `upstreams/linux-runtime.lock.toml` selects the kernel and distribution baseline.

The final Linux source lock must cover both the kernel and userspace. For the userspace image, every installed binary package must resolve to its corresponding source package and the lock must retain:

- package version;
- source package identity/version;
- source archive checksum;
- build dependency closure;
- runtime dependency closure;
- copyright/license information;
- repository snapshot identity.

Wayland, Xwayland where needed, Mesa, PipeWire, D-Bus, XDG Desktop Portal and AT-SPI are mandatory integration components, not optional proof-of-concept extras.

User-installed Linux software can be added later through normal package mechanisms, but exported applications still pass through the common host app registry and brokers.

## Darwin/macOS: all available open source plus clean-room replacements

There is no complete open-source macOS source tree. Therefore "all" cannot legally or technically mean copying all of macOS.

`upstreams/darwin-open-source.lock.toml` pins Apple's public `distribution-macOS` inventory as the starting open-source inventory. Every published component selected from that inventory must be recorded by exact tag/commit and reviewed at component/license level.

Application compatibility then requires open or clean-room implementations for the application-facing pieces that are not available as reusable Apple source. The required closure includes:

- Mach ABI behavior;
- Mach-O loading;
- dynamic loader behavior;
- libSystem/libc-compatible behavior;
- Objective-C runtime behavior;
- dispatch semantics;
- CoreFoundation/Foundation-compatible APIs;
- AppKit-compatible APIs;
- graphics translation;
- audio translation;
- security/keychain-compatible surfaces where legally implementable;
- semantic accessibility translation.

Proprietary macOS frameworks, a macOS system image or unlicensed Apple binaries cannot satisfy the source-completeness gate.

## Release states

The source system has intentionally separate states:

- **planned**: component is identified but not immutably pinned/built;
- **inventory-pinned**: source inventory root is pinned, but full fetch/build/runtime validation is unfinished;
- **ready**: the exact source closure was fetched, licenses/provenance generated, build reproduced, runtime tested and accessibility validated.

`release_complete=true` is forbidden until all required entries are `ready` and all source references are immutable.

## CI gates

The `Runtime source completeness` workflow performs four independent checks:

1. validates that every required logical Linux/Android/Darwin source class exists in the inventory;
2. validates family lockfiles and rejects false ready states;
3. tests the Rust source-completeness proof contract;
4. tests that `aw-app-compat` release admission accepts only a complete source proof from the matching runtime family.

Compilation or application launch alone is therefore not evidence of complete compatibility.

## Next implementation steps

1. generate a resolved Android Repo manifest from the pinned Android 17 security baseline;
2. pin the Linux Debian snapshot and generate the initial package-to-source closure;
3. walk the pinned Apple OSS distribution inventory and resolve every referenced component tag to a commit;
4. pin Darling/GNUstep compatibility dependencies individually after license review;
5. create deterministic source-cache layouts and offline build inputs;
6. generate SPDX/CycloneDX SBOM plus project provenance;
7. build minimal Linux and Android guest images;
8. begin Darwin Mach-O/CLI compatibility using only approved open/clean-room components;
9. connect all three to the common host window/file/URL/clipboard/audio/network/accessibility brokers;
10. run real application and blind-user conformance tests before any runtime is called release-complete.
