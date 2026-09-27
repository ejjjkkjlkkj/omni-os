# OMNI Physical Operator Session

## Goal

OMNI uses a real interactive Windows account for the physical desktop and reserves kernel-level authority for OMNI Kernel.

This replaces the operational model:

```text
interactive user -> PsExec -> LocalSystem process
```

with:

```text
physical login
    |
    v
OMNI-Operator
interactive session
    |
    +-- accessibility UI / speech / braille / keyboard
    +-- administration console
    +-- security confirmation UI
    |
    v
OMNI Kernel
kernel authority / privileged primitives
```

## Physical account

Canonical name:

```text
OMNI-Operator
```

Properties:

- real local Windows user account;
- interactive physical logon;
- member of the local Administrators group;
- dedicated profile;
- no dependency on PsExec;
- no LocalSystem desktop;
- no service desktop;
- no hidden password embedded in OMNI;
- local accessibility stack always available in the user's session.

## Why not LocalSystem as the desktop account

LocalSystem is an internal Windows account used by the operating system and services. It is not the physical-user identity OMNI should expose as its permanent desktop.

OMNI therefore separates:

```text
WHO IS OPERATING
    = OMNI-Operator

WHO OWNS KERNEL AUTHORITY
    = OMNI Kernel
```

## Privilege model

The interactive account owns user intent.

The kernel owns privileged enforcement.

```text
OMNI-Operator
    |
    | semantic privileged request
    v
Trusted OMNI authorization path
    |
    | approved capability
    v
OMNI Kernel
    |
    +-- memory / driver primitives
    +-- disk primitives
    +-- firmware/NVRAM mediation
    +-- device/IOMMU policy
    +-- privileged evidence
    +-- reboot/recovery transitions
```

The normal desktop must not be converted into an unrestricted kernel-equivalent security context.

## User-session permanence

"Permanently logged in" means that the physical OMNI session is the long-lived operator session while the machine is in use.

It does not require:

- a SYSTEM desktop;
- an interactive service;
- a PsExec child process;
- a generic SYSTEM shell;
- storing a plaintext auto-logon password.

A future OMNI login component may implement a dedicated accessible sign-in flow, but it must preserve credential secrecy and trusted-path properties.

## Accessibility

The physical session is the primary accessibility surface.

It must provide:

- speech from login onward;
- braille from login onward;
- keyboard-only control;
- semantic focus/state/action reporting;
- trusted-security announcements;
- secret-field suppression;
- accessible elevation decisions;
- accessible kernel error/recovery state.

## Kernel contract

OMNI Kernel receives bounded operation identifiers, not arbitrary command strings.

Example operations:

```text
QUERY_PLATFORM_STATE
VERIFY_PHYSICAL_DISK
STAGE_SIGNED_UEFI_ARTIFACT
SET_ONE_SHOT_BOOT
READ_TPM_STATE
READ_SECURE_BOOT_STATE
LOAD_APPROVED_DRIVER
APPLY_DEVICE_POLICY
ENTER_RECOVERY
REBOOT_FIRMWARE
```

Each operation must define:

- caller;
- target;
- required authority;
- validation;
- authorization policy;
- rollback;
- evidence;
- accessible result.

## PsExec migration

During migration, PsExec can remain only as an external comparison/recovery tool.

The target state is:

```text
OMNI-Operator + OMNI Kernel
        |
        +-- no PsExec dependency for normal operation
```

PsExec removal gate:

1. physical OMNI-Operator login proven;
2. accessible login proven;
3. kernel request channel proven;
4. unauthorized caller denied;
5. privileged disk operation proven;
6. UEFI/NVRAM operation proven;
7. reboot survival proven;
8. crash/recovery proven;
9. evidence binding proven;
10. no regression of speech/braille.
