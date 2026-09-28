# Roadmap

## Phase 0 - Repository bootstrap

- [x] Rust workspace
- [x] Kernel boot-data contract
- [x] Accessibility semantic primitives
- [x] Accessibility invariant tests
- [x] Architecture specification
- [x] Cross-platform x64 CI
- [x] Committed Cargo lockfiles and reproducibility gate
- [x] CodeQL, RustSec, fuzzing, coverage and dependency review
- [x] SBOM/checksum/provenance pipeline
- [x] Accessible recovery contract with deterministic keyboard semantics and nonvisual delivery evidence
- [x] Trial-generation attempt consumption before control transfer so power loss cannot create infinite retry loops
- [x] Recovery/boot architecture research covering WinRE, GRUB, systemd/BLS, OSTree and ChromiumOS concepts
- [x] Ten-year persistent-format/accessibility compatibility policy

## Phase 1 - UEFI bring-up

- [x] Add an x86-64 UEFI boot application
- [x] Build the `.efi` executable in CI
- [x] Print deterministic UEFI console diagnostics
- [x] Boot the EFI stage automatically under OVMF/QEMU
- [x] Read and validate availability of the UEFI memory map
- [x] Detect GOP and current display mode
- [x] Capture framebuffer address/size when GOP exposes direct framebuffer access
- [x] Locate the ACPI RSDP through the UEFI configuration table
- [x] Validate ACPI RSDP signature, declared length and checksums
- [x] Produce an aligned GPT disk image with a FAT32 EFI System Partition
- [x] Boot-test the generated raw disk image under OVMF/QEMU
- [x] Exit UEFI Boot Services after dropping boot-services resources
- [x] Prove post-firmware execution through debugcon in CI
- [x] Define and validate the owned kernel handoff structure
- [x] Split the post-firmware stage into a freestanding x86-64 kernel
- [x] Load the flat kernel from the ESP at the fixed base its `AWKN` header declares
- [x] Execute the separate native kernel after ExitBootServices
- [x] Write directly to the framebuffer from the native kernel

## Phase 2 - Generic x64 kernel foundation

- [x] Detect AMD, Intel and unknown x86-64 CPU vendors with CPUID
- [x] Validate common APIC/SSE2/long-mode boot baseline
- [x] Detect x2APIC and invariant-TSC capabilities
- [x] Parse ACPI MCFG and pass PCIe ECAM regions to the kernel
- [x] Enumerate PCIe configuration space through ECAM
- [x] Keep PCI mechanism #1 (CF8/CFC) as a legacy fallback
- [x] Classify NVMe, AHCI, xHCI and HDA controllers by PCI class
- [x] Decode PCI I/O, MMIO32 and MMIO64 BARs
- [x] Transfer the complete final UEFI memory map to the kernel
- [x] Physical page allocator
- [x] Virtual-memory manager: kernel-owned identity map with W^X, NX and a
      guard page, each proved by a deliberate fault under QEMU/OVMF
- [x] Enable and verify CR0.WP, EFER.NXE, CR4.SMEP/SMAP/UMIP
- [x] GDT/IDT, segment reload, TSS/IST and exception handling, with recoverable
      faults and a real #DF on IST1
- [x] APIC timer interrupts: periodic delivery, monotonic tick counter, EOI, and
      the negative test that masking the vector stops it
- [x] IOAPIC/MSI routing for device interrupts: a real device IRQ routed by the
      MADT's interrupt source overrides through an I/O APIC, and an MSI written
      straight into the local APIC, each proved by delivery plus a mask test
- [x] SMP bring-up: INIT-SIPI-SIPI through a real-mode trampoline, each AP
      installing its own GDT, TSS and IST, proved by every AP reporting the
      APIC ID it read from its own local APIC and tables distinct from all
      others'
- [ ] SMP validation on both AMD and Intel test profiles
- [x] Scheduler: cooperative and timer-preemptive, kernel and CPL3 threads (`AW_PREEMPT_PROOF_OK`, `AW_RING3_PREEMPT_PROOF_OK`)
- [x] User/kernel privilege separation: CPL3, versioned `syscall`/`sysret` ABI, SMAP-checked copies, scratch registers cleared on return
- [x] IPC and handle/object model: reference-counted channels reached only through per-process, generation-checked, rights-carrying handles (`AW_IPC_PROOF_OK`, `AW_HANDLE_SECURITY_PROOF_OK`)

## Phase 3 - Generic physical PC minimum

- [ ] ACPI table enumeration beyond MCFG
- [ ] MADT/APIC topology parsing
- [ ] PCI bridge-aware enumeration
- [x] NVMe controller initialization and identify
- [x] NVMe read/write: I/O queue pair, READ of a known sector, WRITE + read-back on a scratch disk
- [ ] AHCI controller initialization
- [ ] AHCI/SATA read/write
- [ ] GPT parser/writer with safety checks
- [ ] xHCI controller initialization
- [ ] USB hub enumeration
- [ ] USB HID keyboard
- [ ] USB HID pointer baseline
- [x] Basic framebuffer console
- [x] ACPI power off/reboot: FADT + `\_S5`, S5 soft-off and FADT reset register, each proved by the VM's own behaviour
- [ ] Physical boot validation on at least one AMD x64 PC
- [ ] Physical boot validation on at least one Intel x64 PC

## Phase 4 - Installable generic x64 system

- [ ] Produce one AMD/Intel x64 installation image
- [ ] Accessible USB installer
- [ ] Disk selection with spoken device identity
- [ ] System partition creation
- [ ] Filesystem implementation/selection
- [ ] Install system image
- [ ] UEFI boot entry creation
- [ ] One-shot boot request that cannot silently become the permanent default
- [ ] Independent signed local Recovery Core, not dependent on normal mutable OS configuration
- [ ] Signed external recovery media path when local boot/recovery metadata is damaged
- [ ] Recovery from corrupted/ambiguous redundant boot-state copies without guessing
- [ ] First boot from physical NVMe/SATA SSD
- [ ] Recovery environment
- [ ] Recovery keyboard flow with no destructive timeout and explicit reinstall confirmation
- [ ] Installer fallback to GOP without accelerated GPU driver

## Phase 5 - Desktop and accessibility

- [ ] Compositor
- [ ] Accessible native UI toolkit
- [ ] Semantic tree service
- [ ] Keyboard-only shell
- [ ] Speech service
- [ ] Native screen reader
- [ ] Braille transport layer
- [ ] Accessible settings, file manager and terminal
- [ ] Accessibility available in boot, installer, recovery and first boot

## Phase 6 - Networking, audio, graphics and updates

- [ ] Generic HDA controller baseline
- [ ] Common Ethernet driver families
- [ ] AMD GPU driver family
- [ ] Intel GPU driver family
- [ ] Wi-Fi architecture and first supported families
- [ ] Bluetooth architecture
- [ ] Audio stack including vendor DSP extensions later
- [ ] Network stack
- [ ] Package manager
- [ ] Atomic updates
- [ ] Rollback/snapshots
- [ ] Optional signed network remediation that stages a new generation instead of patching known-good content in place
- [ ] Power-loss fault injection at every update and boot-state persistence boundary
- [ ] Migration fixtures for every persistent format still inside the ten-year support window

## Phase 7 - Unified application compatibility

### Common host integration

- [x] Define one host integration contract for native, Android, Linux and Darwin applications
- [x] Require launcher, host window, clipboard, notifications, file/URL portals, audio, network and accessibility bridges before an application is called host-integrated
- [x] Define hardware-isolated VM boundary for Linux and Android and userspace compatibility boundary for Darwin
- [x] Require an explicit foreign-ISA translation capability for ARM64 application code on the x86-64 host
- [x] Define complete runtime source/provenance contract in `aw-runtime-sources`
- [x] Bind release-grade `aw-app-compat` admission to a matching complete source-family proof
- [x] Add CI gates that reject missing source classes, false ready states and moving/unpinned release inputs
- [x] Pin initial crosvm source baseline for the compatibility VM substrate
- [x] Select QEMU TCG as the initial AArch64-to-x86-64 translation baseline
- [ ] Implement host app registry with stable IDs across native/Android/Linux/Darwin origins
- [ ] Implement capability-token broker protocol with explicit major/minor versioning
- [ ] Implement host window broker
- [ ] Implement file and URL portals
- [ ] Implement clipboard and notification brokers
- [ ] Implement audio and network brokers
- [ ] Implement common runtime lifecycle and crash containment
- [ ] Implement AArch64-to-x86-64 translation adapters without weakening isolation
- [ ] Build real app conformance corpus and blind-user accessibility corpus

### Linux application runtime

- [x] Select real Linux-in-VM architecture instead of sharing the host kernel
- [x] Select Linux 6.18 LTS source baseline and Debian 13.6 userspace family
- [x] Require binary-package-to-source-package closure, licenses, SBOM and provenance for every shipped userspace binary
- [x] Require Wayland, Xwayland compatibility, Mesa, PipeWire, D-Bus, XDG Desktop Portal and AT-SPI integration classes
- [ ] Pin immutable Debian repository snapshot
- [ ] Generate complete initial Debian binary-to-source closure
- [ ] Build minimal amd64 Linux guest image reproducibly
- [ ] Boot Linux guest under the selected compatibility VMM
- [ ] Export one Wayland application as an ordinary host window
- [ ] Translate AT-SPI semantics into the native accessibility tree
- [ ] Add Linux package-manager install/export/uninstall flow
- [ ] Validate x86-64 Linux application corpus
- [ ] Validate ARM64 Linux application translation corpus

### Android application runtime

- [x] Select AOSP rather than a proprietary Android distribution
- [x] Pin the complete Android 17 security Repo manifest baseline
- [x] Require every project in the resolved AOSP manifest to be recorded by exact commit
- [x] Require ART, Binder, Bionic, framework, graphics, media/audio, package/activity, permissions/security and accessibility source classes
- [x] Explicitly exclude proprietary Google packages from the assumed open-source baseline
- [ ] Perform complete AOSP source synchronization from the pinned manifest
- [ ] Commit/generated-store resolved AOSP manifest containing every project commit
- [ ] Build x86-64 Android guest image reproducibly
- [ ] Boot Android guest under the selected compatibility VMM
- [ ] Install APK and export individual activity as ordinary host app/window
- [ ] Bridge Android intents to host file/URL handlers
- [ ] Bridge Android notifications, clipboard, audio and networking
- [ ] Translate Android accessibility nodes/events into native accessibility semantics
- [ ] Add Android Native Bridge adapter for ARM64-only native libraries
- [ ] Validate x86-64/DEX Android application corpus
- [ ] Validate ARM64-native Android application corpus

### Darwin/macOS application compatibility

- [x] Define clean-room userspace compatibility architecture; no macOS VM/system image requirement
- [x] Pin Apple `distribution-macOS` open-source inventory baseline
- [x] Require walking every Apple-published project in that inventory and resolving tags to exact commits
- [x] Select Darling, GNUstep/libobjc2 and Apple OSS components as component-level candidates/references subject to license review
- [x] Explicitly forbid proprietary macOS frameworks/system images from satisfying source-completeness gates
- [ ] Resolve and record every project in the pinned Apple OSS inventory
- [ ] Complete component-level license review and source-use classification
- [ ] Pin Darling/GNUstep/libobjc2 source revisions
- [ ] Implement initial x86-64 Mach-O loader
- [ ] Implement dynamic-loader and Darwin/Mach/POSIX compatibility services
- [ ] Bring up Objective-C runtime and CLI compatibility
- [ ] Implement Foundation/CoreFoundation compatibility coverage
- [ ] Implement AppKit-compatible host-window integration
- [ ] Implement graphics/audio/security compatibility surfaces
- [ ] Implement Darwin accessibility semantic bridge
- [ ] Validate x86-64 Mach-O application corpus
- [ ] Validate ARM64 Mach-O translation corpus

### Other compatibility

- [ ] Win32 ABI/API compatibility program
- [ ] Windows application test corpus
- [ ] Web/PWA runtime

## Definition of success

The project does not claim to exceed Windows 11 until reproducible benchmarks demonstrate improvements in selected areas such as accessibility coverage, idle resource use, recovery, update reliability, input latency and security isolation while maintaining useful application and hardware compatibility.

Compatibility is not considered complete because an application launches. Android, Linux and Darwin applications must behave as first-class host applications and retain native screen-reader semantics without entering a separate guest desktop. Release-grade compatibility also requires complete immutable source closure, license/provenance records and reproducible runtime builds.

Recovery is not considered better merely because it has more options: it must demonstrably survive boot/update corruption, remain independently recoverable, preserve a known-good generation, and be fully operable by a blind user without visual assistance.
