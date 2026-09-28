# OMNI Protected Read-Only Model

## Core rule

Any state that the physical user must be able to inspect but must not be able to modify directly is exposed as **read-only**.

```text
USERSPACE VIEW
    = READ

DIRECT USERSPACE MUTATION
    = DENY

AUTHORIZED CHANGE
    = OMNI Kernel capability
```

## Protected state classes

Examples of state that should normally be read-only to the interactive user session:

- kernel policy;
- boot trust state;
- Secure Boot state;
- TPM measurements and attestation evidence;
- approved firmware hashes;
- protected device policy;
- IOMMU/DMA policy;
- kernel module allowlist;
- update trust roots;
- rollback counters;
- recovery policy;
- protected audit/evidence chains;
- root-of-trust metadata;
- security mode configuration;
- trusted accessibility path configuration.

## Mutation path

A protected object can be changed only through a bounded privileged operation.

```text
OMNI-Operator
    |
    | read / inspect
    v
READ-ONLY PROTECTED STATE

OMNI-Operator
    |
    | explicit semantic change request
    v
policy + authorization + accessible confirmation
    |
    v
OMNI Kernel
    |
    | validated mutation
    v
protected state
    |
    v
verification + evidence + rollback state
```

## Read-only does not mean invisible

Protected state should remain inspectable where disclosure is safe.

The user must be able to know:

- current value;
- source;
- trust status;
- last change;
- who/what changed it;
- whether the value is measured or attested;
- whether recovery/rollback is available.

Sensitive values are redacted rather than hidden behind unrestricted write access.

## Access classes

| Class | User-space access | Mutation |
|---|---|---|
| PUBLIC | read | policy dependent |
| PROTECTED | read | kernel-mediated only |
| SENSITIVE | redacted read | kernel-mediated only |
| SECRET | metadata only | never exposed directly |
| APPEND_ONLY | read | append through trusted path only |

## Files and storage

Where protected state is materialized as files:

- ordinary user ACL: read-only;
- no direct write/delete permission;
- integrity hash or signature where appropriate;
- immutable/versioned replacement instead of in-place editing for critical policy;
- recovery copy separated from active state;
- append-only logs where feasible;
- raw disk write path blocked from ordinary user-space.

## Kernel API rule

The kernel should expose semantic operations such as:

```text
SET_BOOT_POLICY
INSTALL_TRUSTED_KEY
APPLY_DEVICE_POLICY
STAGE_SIGNED_UPDATE
ROTATE_RECOVERY_STATE
APPEND_SECURITY_EVIDENCE
```

and not generic primitives such as:

```text
WRITE_ANY_FILE
WRITE_ANY_REGISTRY_KEY
WRITE_ANY_PHYSICAL_ADDRESS
RUN_ARBITRARY_KERNEL_COMMAND
```

## Accessibility

Read-only protected state must remain fully accessible.

The user can inspect it through:

- speech;
- braille;
- keyboard navigation;
- semantic name/value/state;
- machine-readable evidence.

For a protected setting, the UI must announce that it is read-only and explain the authorized path required to change it.

Secret state must never be spoken or rendered in plaintext merely because the user has read access to the surrounding object.

## Recovery

If protected state becomes invalid:

1. ordinary user-space still cannot overwrite it directly;
2. OMNI enters a controlled recovery path;
3. recovery uses signed/reference state;
4. the user receives an accessible explanation;
5. restoration emits evidence;
6. rollback is verified before returning to normal operation.

## Invariant

```text
READABILITY does not imply MUTABILITY.

VISIBILITY does not imply AUTHORITY.

ADMINISTRATOR does not imply unrestricted write access
to OMNI protected state.
```
