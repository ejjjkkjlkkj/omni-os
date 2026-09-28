# Generic x64 hardware compatibility

Accessible Windows targets one generic x86-64 installation image for modern PCs. It is not tied to ASUS or to any single OEM. AMD and Intel systems use the same x86-64 UEFI image; CPU vendor and devices are discovered at runtime.

## Product rule

There is one x64 OS image and one x64 kernel ABI.

The boot path must never require a vendor-specific ASUS, AMD or Intel firmware image. OEM-specific support is layered on top of standard platform discovery.

## Baseline platform contract

The generic PC baseline is:

- x86-64 long mode;
- UEFI firmware;
- ACPI platform description;
- PCI/PCIe configuration and enumeration;
- ACPI MCFG / PCIe ECAM as the preferred modern PCIe configuration path;
- PCI configuration mechanism #1 (CF8/CFC) as a legacy segment-zero fallback;
- APIC/x2APIC interrupt architecture where available;
- invariant/constant TSC detection with safe timer fallbacks;
- UEFI GOP framebuffer as the universal early-display fallback;
- GPT storage layout;
- NVMe over PCIe as the primary SSD path;
- AHCI/SATA as the secondary storage path;
- xHCI as the primary USB host-controller path;
- USB HID keyboard and pointer classes;
- PCI High Definition Audio as the first generic audio-controller path.

These interfaces are intentionally CPU-vendor neutral. CPU-specific code is selected only after CPUID/vendor/feature discovery.

## AMD and Intel CPU handling

The kernel queries CPUID rather than assuming one vendor.

Common x86-64 code handles:

- paging;
- privilege levels;
- syscall/sysret where available and selected;
- XSAVE/FPU state based on CPUID;
- APIC/x2APIC;
- TSC capability detection;
- SMP discovery through ACPI.

Vendor-specific modules may then enable optional behavior:

- AMD-specific MSRs/features;
- Intel-specific MSRs/features;
- power/performance controls;
- IOMMU support (AMD-Vi / Intel VT-d);
- virtualization extensions (AMD-V / Intel VT-x) when useful later.

Unknown x86-64 CPUs fall back to the common feature-detected path instead of failing because the vendor is unfamiliar.

## Driver matching model

Drivers are selected by bus/class identifiers, not by laptop model name.

### PCI/PCIe

Match in this order where appropriate:

1. class/subclass/programming-interface for standards-compliant controllers;
2. vendor/device ID for hardware that requires vendor-specific behavior;
3. subsystem vendor/device ID only for machine-specific quirks.

The kernel currently receives validated MCFG ECAM regions from the UEFI loader, scans PCIe through ECAM first, and retains CF8/CFC only as a compatibility fallback. Standard PCI BAR decoding supports I/O BARs plus 32-bit and 64-bit MMIO BARs; this is the prerequisite for mapping NVMe, xHCI, HDA and other controller register windows.

### USB

Prefer standard USB class drivers first:

- HID;
- mass storage;
- hubs;
- audio where practical;
- CDC/network classes where applicable.

Vendor/product-specific USB drivers are a second layer.

### ACPI

ACPI namespace objects describe batteries, buttons, lid, thermal zones, PCI routing, sleep/power state and many platform devices. OEM ACPI methods are handled by isolated quirk modules and must not define the generic boot contract.

## Initial driver coverage

### Required for the first broadly bootable ISO

- UEFI GOP framebuffer;
- ACPI parser/enumerator;
- PCI/PCIe enumerator;
- APIC/x2APIC and timers;
- NVMe;
- AHCI/SATA;
- xHCI;
- USB HID keyboard;
- USB HID mouse/touchpad baseline;
- generic GPT support;
- basic power-off/reboot paths.

### Next coverage tier

- Intel HDA-compatible controller;
- common Ethernet families;
- virtio devices for virtual-machine CI;
- IOMMU discovery;
- laptop battery/lid/power-button ACPI;
- USB audio and storage classes.

### Vendor-specific tier

Graphics acceleration, Wi-Fi, Bluetooth, advanced laptop audio DSPs, fingerprint readers and OEM hotkeys require dedicated driver families. They must not be prerequisites for installation because the OS retains generic fallbacks where technically possible.

## Graphics strategy

The installer and recovery environment must always be able to operate using the UEFI GOP framebuffer when firmware provides it.

Native accelerated graphics is added separately:

- AMD GPU driver family;
- Intel GPU driver family.

## Validation policy

Automated GitHub validation must exercise at least one Intel CPU model, one AMD CPU model and a generic/unknown-compatible path where practical. Every profile boots the same raw GPT/ESP image. VM validation does not replace physical boot testing: release-quality hardware support requires separate real-PC validation on both AMD and Intel x64 systems.
