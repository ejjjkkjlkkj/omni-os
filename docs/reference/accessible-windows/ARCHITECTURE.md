# Architecture

## Product target

Accessible Windows is intended to become a standalone operating system for physical x64 PCs, not a theme, Windows modification, virtual-machine image, or application shell.

The hardware contract is deliberately **x86-64 only**, but vendor-neutral inside that architecture:

- AMD64/x86-64 CPUs from AMD or Intel;
- one generic x64 kernel and installation image, not separate AMD and Intel editions;
- runtime CPU feature discovery through CPUID;
- UEFI firmware;
- ACPI platform description;
- PCI/PCIe discovery;
- USB HID input;
- UEFI framebuffer for first graphics;
- NVMe first, SATA/AHCI second;
- installation to a GPT disk with an EFI System Partition.

ARM and ARM64 are explicitly out of scope. The project will not maintain ARM bootloaders, kernels, CI runners or release artifacts. This decision concentrates engineering and validation on the x64 PC ecosystem.

The project must not depend on a particular OEM such as ASUS. OEM-specific behavior is represented by optional driver/quirk modules. Standard hardware is matched by PCI/USB/ACPI classes and identifiers.

Virtual machines are used for deterministic CI and debugging, but every subsystem must be designed for eventual execution on physical AMD and Intel x64 hardware.

## Layering

```text
UEFI firmware
    |
Generic x64 boot application / installer
    |
Kernel + x64 HAL
    |-- CPUID / AMD-or-Intel feature discovery
    |-- memory management
    |-- scheduler
    |-- interrupts / timers
    |-- IPC / object model
    |-- security
    |
Driver services
    |-- ACPI / PCI / PCIe
    |-- NVMe / AHCI
    |-- xHCI / USB / HID
    |-- network
    |-- HDA / vendor audio extensions
    |-- GOP fallback / AMD GPU / Intel GPU
    |
System services
    |-- filesystem
    |-- networking
    |-- package/update service
    |-- accessibility service
    |
Compositor + Accessible UI framework
    |
Desktop / shell / applications
    |
Compatibility environments
```

See `HARDWARE-COMPATIBILITY.md` for the detailed generic PC driver strategy.

## Kernel direction

Rust is the default language for newly designed privileged components. Unsafe Rust is not globally forbidden forever because hardware access and context switching will require narrowly scoped unsafe code, but every unsafe boundary must eventually be isolated, documented and tested. During bootstrap the public contract crates forbid unsafe code entirely.

The kernel ABI must avoid dependencies on the desktop, Win32 compatibility or a specific UI toolkit. It is an x86-64 ABI only; portability to ARM is not a design requirement.

CPU-vendor-specific behavior must not leak into the generic kernel contract. The common x86-64 path detects features at runtime and enables AMD- or Intel-specific modules only when needed.

## Boot contract

The boot stage must eventually provide the kernel with:

- validated memory map;
- framebuffer description when available;
- ACPI RSDP location;
- UEFI system information needed after ExitBootServices only when explicitly retained;
- boot volume identity;
- entropy seed;
- command-line/recovery flags.

`aw-kernel-contract` starts this interface without requiring allocation.

## Storage and installation

The first installable milestone must support:

1. booting from USB or generic x86-64 UEFI installation media;
2. enumerating NVMe devices by standard PCI class/interface;
3. falling back to AHCI/SATA where applicable;
4. reading and writing GPT structures safely;
5. creating or selecting an EFI System Partition;
6. installing boot files and an initial system image;
7. rebooting and starting from the internal disk;
8. preserving a recovery path.

Destructive disk operations must never be enabled until explicit device identity and partition-layout checks exist.

The project will maintain a raw GPT/ESP image for deterministic CI and will add a generic bootable x64 installer ISO once the installer payload exists. Both artifacts use the same kernel and runtime hardware discovery.

## Compatibility strategy

Compatibility is layered rather than built into the kernel:

1. native Accessible Windows APIs;
2. POSIX/Linux workloads through an isolated subsystem;
3. progressively compatible Win32 user-mode environment;
4. web/PWA runtime;
5. Android or other guest environments only after the core OS is stable.

The project will not use leaked proprietary Windows source code to implement compatibility.
