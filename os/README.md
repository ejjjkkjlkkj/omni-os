# Accessible Windows

Accessible Windows is an experimental, clean-room operating-system project for modern x64 PCs.

The target is a real x86-64 UEFI operating system that can eventually boot from USB, install to NVMe/SATA storage, start directly on physical hardware, provide a modern desktop, and treat accessibility as a core system contract rather than an optional application.

## Architecture scope

Accessible Windows is **x86-64/x64 only**. ARM and ARM64 are out of scope for the project, its kernel, boot path, CI matrices and release artifacts. This keeps engineering effort focused on one real PC architecture and allows deeper hardware, driver, compatibility and accessibility validation.

## Project principles

- Accessibility-first: every native interactive control must expose semantic information.
- Physical hardware first-class: virtual machines are test targets, not the final product.
- x64-only: all supported boot, kernel, driver and release paths target x86-64.
- Memory safety where practical: Rust is the default implementation language for new privileged code.
- Clean-room compatibility: do not copy leaked or otherwise unauthorized proprietary Windows source code.
- Measurable quality: performance, boot time, memory use, accessibility and compatibility will be benchmarked.
- Recoverability: updates and system services are designed with rollback and recovery in mind.

## Current status

Bootstrap phase. The repository currently contains:

- an x86-64 UEFI executable under `boot/uefi`;
- a separate x86-64 freestanding kernel, loaded at the fixed base its `AWKN`
  image header declares;
- GDT with segment reload, a full 256-entry IDT, TSS/IST and recoverable
  exception handling;
- kernel-owned page tables enforcing W^X, NX and a guard page below the
  double-fault stack, with CR0.WP and CR4.SMEP/SMAP/UMIP enabled;
- Local APIC timer interrupts with a monotonic tick counter;
- device interrupts routed through an I/O APIC using the MADT's interrupt
  source overrides, and MSI delivered straight into the local APIC;
- SMP bring-up, with every application processor on its own GDT, TSS and IST;
- UEFI memory-map and GOP/display discovery, ACPI/PCIe ECAM enumeration;
- a GPT disk-image builder with a FAT32 EFI System Partition;
- a QEMU/OVMF proof suite that asserts what the CPU actually did, not what
  compiled - see [docs/KERNEL-BOOT-PROOFS.md](docs/KERNEL-BOOT-PROOFS.md);
- `no_std` crates for the kernel/boot contract and the accessibility semantic
  model;
- a native screen-reader announcement engine that turns that semantic model
  into the exact utterance spoken for each control, proven on the boot path by
  voicing the installer's first screen (nonvisual delivery evidence);
- architecture and accessibility specifications;
- x64-only Rust CI on Linux, Windows and Intel macOS.

There is no scheduler, no user mode, no driver stack, no filesystem and no user
interface yet, so there is nothing to install: the generated disk image is a
boot prototype, not an operating-system installer.

## Initial target

- Architecture: x86-64 / x64 only
- Firmware: UEFI
- Boot media: USB / EFI System Partition
- Installation target: NVMe/SATA SSD
- UI: native compositor and accessible UI toolkit
- Compatibility goals: native API first, then progressively Win32, Linux and web workloads

## Repository layout

```text
boot/
  uefi/                 First x86-64 UEFI executable
kernel/
  x86_64/               Freestanding native x86-64 kernel
crates/
  aw-kernel-contract/   Boot and kernel-facing data contracts
  aw-kernel-core/       Kernel handoff validation
  aw-acpi/              ACPI validation primitives
  aw-accessibility/     Semantic accessibility primitives and validation
docs/
  ARCHITECTURE.md
  ACCESSIBILITY.md
  KERNEL-BOOT-PROOFS.md         What counts as evidence, and what is proved today
  FIRMWARE-ACCESSIBILITY-ROADMAP.md  Firmware-stage screen reader: what ships, and the gaps
  PRIOR-ART-AND-SIGNIFICANCE.md What existed before, the standards, and what is new here
  ROADMAP.md
  LEGAL.md
  REAL-HARDWARE-TEST.md
scripts/
  build-uefi-disk.sh    Builds the GPT/FAT32 UEFI disk image
  Invoke-KernelBoot.ps1 Builds one kernel configuration and boots it in QEMU
  Invoke-BootProofs.ps1 Asserts the markers every configuration must produce
  verify-windows.ps1    Host quality gates, then all boot proofs
```

## Build workspace

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Build UEFI executable

### Verify the prototype on Windows

With PowerShell 7, Rust (including `llvm-tools-preview` and the
`x86_64-unknown-none` / `x86_64-unknown-uefi` targets), and QEMU installed:

```powershell
pwsh -NoProfile -File scripts\verify-windows.ps1
```

The script locates the repository relative to itself, checks formatting, runs
workspace checks/tests/Clippy plus Clippy on the kernel crate for every feature
combination it ships, then hands over to `scripts\Invoke-BootProofs.ps1`, which
builds and boots each kernel configuration under QEMU TCG with the bundled EDK2
firmware and asserts its required and forbidden markers. Use `-Qemu` to override
the default `C:\Program Files\qemu\qemu-system-x86_64.exe` path, or `-ProofsOnly`
to skip the host gates. Evidence for each run is kept under
`target/boot-evidence/<configuration>/`.

This virtual-FAT proof suite does not validate the raw GPT image, installation,
physical hardware, or completion of the operating-system roadmap.

```bash
rustup target add x86_64-unknown-uefi
cargo build --manifest-path boot/uefi/Cargo.toml --target x86_64-unknown-uefi --release
```

Expected EFI output:

```text
boot/uefi/target/x86_64-unknown-uefi/release/aw-uefi-boot.efi
```

On Linux with `gdisk`, `dosfstools` and loop-device support, build the GPT/ESP image with:

```bash
./scripts/build-uefi-disk.sh
```

Expected disk image:

```text
build/accessible-windows-uefi-x86_64.img
```

On Windows (or any host with only PowerShell 7, Rust and Python 3, no imaging
tools), build a bootable GPT + FAT16 ESP image and prove it boots under
QEMU/OVMF with:

```powershell
pwsh -NoProfile -File scripts\Build-BootableImage.ps1 -Verify
```

Expected disk image:

```text
dist/accessible-windows-uefi-x86_64.img
```

`-Verify` boots the image and requires the `AW_NATIVE_KERNEL_IDLE` marker with
no CPU exception or panic. The FAT filesystem and GPT wrapper are assembled by
`scripts/build_bootable_image.py` with no external tools. Boot it directly with
`qemu-system-x86_64 ... -drive format=raw,file=dist/accessible-windows-uefi-x86_64.img`,
or write it to a USB stick (for example Rufus in DD/image mode, or `dd`) and
boot it through the firmware's UEFI removable-media entry.

The image is intended for controlled boot testing. It is not yet an installer and must not be written over a disk containing data you need.

## Source policy

This project must remain clean-room. Public documentation, published specifications and appropriately licensed open-source projects may be used according to their licenses. Leaked or unauthorized proprietary operating-system source code must not be copied, translated or imported into this repository.

## License

A project license has not yet been selected. Until one is added, do not assume permission to redistribute repository code outside the rights provided by applicable law.
