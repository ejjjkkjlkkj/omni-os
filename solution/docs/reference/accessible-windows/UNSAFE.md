# Unsafe-code policy

Accessible Windows is Rust-first, but an operating system cannot eliminate every unsafe operation. Firmware transitions, CPU instructions, MMIO, interrupt setup and context switching require carefully bounded unsafe code.

## Rules

- Safe Rust is the default.
- Unsafe code must be isolated at a hardware, firmware or FFI boundary.
- Every unsafe block must have a `SAFETY:` comment describing the invariant relied upon.
- Higher-level policy, accessibility semantics and ordinary system services should remain safe Rust wherever practical.
- CI must exercise unsafe transition paths that can be tested deterministically.

## Current audited boundaries

The initial UEFI stage has two explicit unsafe boundary classes:

1. ACPI RSDP borrowing from the UEFI configuration-table pointer. Only bounded slices are created; signature, checksums and declared length are validated by the safe `aw-acpi` crate before the data is accepted.
2. `uefi::boot::exit_boot_services`. Before the call, boot-services-backed protocol objects and temporary maps are dropped. After the call, the smoke-test path invokes no UEFI Boot Services and reports progress through x86 debugcon.

All parsing and policy above these raw firmware boundaries remains safe Rust.
