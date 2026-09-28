# Real hardware validation

Physical hardware validation is a mandatory release gate for Accessible Windows.
A QEMU/OVMF pass is an integration test, not a substitute for a physical PC boot.

## Current validation levels

| Level | Requirement | Status |
| --- | --- | --- |
| Host toolchain | Rust format/check/test/clippy on Linux, Windows, macOS | PASS |
| Disk structure | Raw GPT image with FAT32 EFI System Partition and verified boot files | PASS |
| Firmware integration | Raw GPT image boot through UEFI/OVMF | PASS |
| Native kernel build | Separate `x86_64-unknown-none` kernel, static flat image, no runtime relocations | PASS |
| Native kernel execution | Bootloader loads kernel into firmware-selected RAM and transfers execution after `ExitBootServices` | PASS |
| Native framebuffer access | Kernel writes a visible early-boot marker without UEFI Boot Services | PASS |
| USB physical boot | The exact CI-produced raw image boots from a dedicated USB device | PENDING |
| Physical PC boot | Kernel executes on real x86-64 UEFI hardware | PENDING |
| Accessible physical proof | Independent non-visual boot feedback usable without sighted assistance | PENDING |
| Internal-disk installation | Installer writes only to an explicitly selected target disk and reboots from it | NOT IMPLEMENTED |

Validated automated boot evidence for commit `9d072749cc21f8b8ea907f23897947807aab3fd9`, CI run `34574731585`:

```text
AW_NATIVE_KERNEL_BUILD_OK bytes=963 start_vma=0x0000000000000000 relocations=none
AW_ESP_KERNEL_VERIFY_OK path=\KERNEL.BIN bytes=963
AW_NATIVE_KERNEL_LOAD_OK address=0xe664000 bytes=963 pages=1 mode=dynamic_pic
AW_MEMORY_MAP_OK entries=131
AW_ACPI_VALIDATE_OK revision=2 length=36
AW_GOP_OK width=1280 height=800 stride=1280 format=Bgr
AW_FRAMEBUFFER_OK address=0x80000000 size=4096000
AW_EXIT_BOOT_SERVICES_OK entries=131
AW_KERNEL_HANDOFF_OK magic=0x41574b484f464631 abi=1 size=72 memory_entries=131 flags=0x1
AW_NATIVE_KERNEL_TRANSFER address=0xe664000 bytes=963
AW_NATIVE_KERNEL_ENTRY_OK
AW_NATIVE_FRAMEBUFFER_WRITE_OK
AW_NATIVE_KERNEL_IDLE
```

This evidence validates the raw GPT/OVMF integration path only. It does **not** count as USB or physical-PC validation.

## Safety rules for the first physical test

1. Use a dedicated disposable USB device. The image write is destructive to that USB device.
2. Do not write the image to the internal SSD/NVMe.
3. Use UEFI boot mode. Legacy BIOS is not a v0.1 target.
4. Disable Secure Boot for the first unsigned engineering builds. Secure Boot support is a later gated milestone.
5. Boot the USB as a live engineering image only. The current image contains no disk installer and must never modify internal storage.
6. Keep recovery media for the host operating system available before hardware experiments.
7. Record the exact Git commit, CI run ID, machine model, firmware version, CPU, RAM, GPU, storage controller, and observed result.

## Required proof for a physical PASS

A physical test is PASS only when all applicable items below are observed from the same build:

- firmware loads `EFI/BOOT/BOOTX64.EFI` from the USB ESP;
- bootloader finds and reads `KERNEL.BIN`;
- ACPI RSDP validates;
- GOP/framebuffer discovery succeeds or an explicitly supported fallback is used;
- `ExitBootServices` succeeds;
- control reaches the separate native kernel;
- the native kernel reaches its idle state without reboot/triple fault;
- the machine can be powered off or reset safely;
- no internal disk write occurs.

## Accessibility gate

The current magenta framebuffer marker is useful for automated and sighted diagnostics but is **not** sufficient for an accessibility-first physical PASS.

Before calling the early hardware path accessible, the project must provide a non-visual proof channel. Preferred direction:

1. native audio discovery and an early audible boot pattern;
2. then speech output as part of the accessibility core;
3. serial/debug output remains an engineering fallback, not the user-facing accessibility mechanism.

Until a non-visual channel exists, physical boot may validate hardware execution, but accessibility validation remains PENDING.

## Test record template

```text
Commit:
CI run:
Image SHA-256:
Machine:
Firmware/UEFI version:
Secure Boot: off/on
CPU:
RAM:
GPU:
Storage controller:
USB device:
Bootloader reached: yes/no
ACPI validated: yes/no
Framebuffer reached: yes/no
Native kernel reached: yes/no
Non-visual feedback reached: yes/no
Internal disk unchanged: yes/no
Result: PASS/FAIL
Notes:
```
