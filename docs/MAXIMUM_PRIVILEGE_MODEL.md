# OMNI Maximum Privilege Model

OMNI must be capable of operating at the highest privilege required by each layer, while preventing that authority from becoming ambient or permanent.

## Principle

Maximum privilege is a controlled capability, not the default execution state.

The platform has multiple privilege domains rather than one universal ladder:

- firmware and platform roots;
- UEFI Boot/Runtime Services;
- MM/SMM where the platform exposes it;
- hypervisor/kernel/ring-0;
- SYSTEM/root;
- privileged services;
- ordinary user/workload contexts.

A component receives only the authority needed for a bounded operation, for the shortest practical time.

## OMNI privilege classes

| Class | Typical authority | Examples |
|---|---|---|
| P0 | Unprivileged | ordinary app, parser, UI |
| P1 | Service-scoped | network, audio, update helper |
| P2 | Administrator/root | system configuration, protected files |
| P3 | Kernel/ring-0 | driver, IOMMU, memory manager |
| P4 | Pre-OS/UEFI | boot manager, firmware protocols |
| P5 | Platform-root | MM/SMM, hardware RoT, flash policy where controllable |

P5 is not assumed to be available or trustworthy on arbitrary proprietary hardware.

## Maximum Privilege Mode

A privileged operation must define:

1. required privilege class;
2. actor identity;
3. target object;
4. exact requested capability;
5. reason;
6. duration/scope;
7. rollback or recovery action;
8. evidence emitted;
9. accessible confirmation path;
10. secret-redaction policy.

## Elevation contract

```text
UNPRIVILEGED
   -> request
   -> policy evaluation
   -> accessible trusted confirmation
   -> temporary capability
   -> privileged action
   -> verification
   -> evidence
   -> capability revoked
   -> UNPRIVILEGED
```

No silent permanent elevation.

## Windows physical-lab rule

For Windows hardware operations that genuinely require the highest local OS authority, the supported lab identity is LocalSystem (SID S-1-5-18), launched through the approved PsExec path and verified before the operation.

This does not imply that every task should run as SYSTEM.

## GitHub Actions rule

Repository automation must never use maximum token permissions by default.

Forbidden as a baseline:

- `permissions: write-all`;
- privileged secrets exposed to forked pull requests;
- untrusted code executing before privilege/secrets are gated;
- self-hosted privileged runners for untrusted PR code.

A job that needs write access must declare only the specific permission required for that job.

## Accessibility requirement

Maximum privilege must remain fully operable without sight.

Privileged prompts require:

- trusted speech state;
- trusted braille state;
- keyboard-only control;
- explicit action and target announcement;
- non-spoofable security context;
- secret-safe entry behavior;
- spoken/braille success, failure and rollback result.

No destructive privileged action may rely on color, pointer-only UI, or silent timeout.

## Remote-control rule

Remote control does not automatically grant maximum privilege.

Remote privileged operations require an additional authorization decision, device/session identity, and a policy-controlled trusted path. Local physical-presence requirements may be imposed for destructive or root-of-trust changes.

## Recovery rule

If elevation fails part-way through an operation, OMNI must:

1. stop further privileged mutation;
2. preserve accessible diagnostics;
3. preserve bounded evidence;
4. attempt defined rollback;
5. enter accessible recovery when state cannot be proven safe.

## Evidence

Every maximum-privilege operation should emit at minimum:

```text
event_id
boot_id/session_id
actor
privilege_class
requested_capability
target
policy_id
authorization_result
start_state
end_state
result
rollback_result
evidence_hash
```

Secrets must never be included in plaintext evidence.
