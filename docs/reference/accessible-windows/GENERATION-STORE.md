# Accessible Windows Generation Store

Status: target architecture. The current physical A/B storage design remains the bootstrap implementation until the block layer, authenticated object store, crash-consistency rules, and recovery path are proven.

## Goal

Accessible Windows should not depend permanently on two complete mutable system partitions. The target is an immutable, content-addressed generation model in which each bootable generation is a small authenticated composition of reusable objects.

The design goal is not to reproduce NixOS, OSTree, ChromeOS, Android, Fuchsia, or macOS formats. Those systems are references for properties we want to prove independently: atomic deployment, rollback, content integrity, isolation of mutable state, and recoverability.

## Core invariants

1. A running generation is immutable.
2. Installation of a new generation never mutates the generation currently executing.
3. Executable system content is identified by cryptographic digest and authenticated before execution.
4. Mutable user/machine state is separate from executable system objects.
5. Boot selection is an atomic metadata operation after all referenced objects are durable and verified.
6. Rollback protection is explicit and monotonic; older signed software is not automatically acceptable.
7. At least one previously successful generation remains recoverable until the new generation is proven healthy.
8. Accessibility is part of boot health. A generation that strands a blind user is not considered successful.
9. Recovery is independently authenticated and must not depend on the currently selected generation.
10. Parsers and boot selectors fail closed on malformed, ambiguous, overlapping, truncated, or inconsistent metadata.

## Logical storage model

The eventual disk layout can remain small:

- `AW_ESP`: minimal UEFI boot entry and authenticated boot metadata locator.
- `AW_BOOTSTATE`: redundant atomic records selecting a generation and tracking trial/success state.
- `AW_STORE`: immutable content-addressed objects and generation manifests.
- `AW_RECOVERY`: independently signed accessible recovery environment.
- `AW_STATE`: encrypted mutable user and machine state.

The current physical `AW_BOOT_A/B` and `AW_SYSTEM_A/B` layout is an implementation stage, not a permanent ABI commitment.

## Objects

Objects are immutable byte sequences addressed by a cryptographic digest. An object may contain a kernel, service executable, driver package, system application, resource bundle, configuration schema, or another authenticated structure.

The first Rust model uses a 32-byte `ObjectId` but deliberately does not yet define serialization or perform hashing. The on-disk format must version the digest algorithm explicitly; no parser may infer algorithms from length alone.

Objects are written using a temporary/unreachable state, fully flushed, verified, and only then made reachable by a generation manifest. Garbage collection may remove an object only when no retained generation, recovery image, update transaction, or pinned diagnostic snapshot references it.

## Generation composition

A generation has a monotonically identified generation number, an anti-rollback index, and references to core objects. The first semantic model requires these roles before boot readiness:

- kernel;
- storage service;
- input service;
- audio service;
- accessibility broker;
- speech service;
- security service;
- update service.

This list is intentionally stricter than a conventional graphical boot definition. Input + audio + accessibility broker + speech are part of the minimum recovery path for blind users.

Drivers and optional applications will use separate package/index structures rather than forcing one singleton entry per hardware device into the root generation manifest.

## Trust layers

Boot readiness must eventually require independent proof tokens in this order:

1. structural parse proof;
2. object-digest proof;
3. manifest-signature proof;
4. anti-rollback proof;
5. dependency/composition proof;
6. platform-policy proof;
7. runtime health proof after trial boot.

`aw-generation::BootCandidate` currently proves only semantic composition and anti-rollback policy. It must never be treated as a signature or digest verification result.

## Atomic update transaction

A future updater should implement this state machine:

1. Resolve a new signed generation manifest.
2. Reject unsupported architecture, bootloader requirements, key policy, or rollback index.
3. Download only missing immutable objects.
4. Verify object sizes and digests before admission to `AW_STORE`.
5. Flush every newly admitted object.
6. Verify the complete manifest and all referenced objects again from the store.
7. Commit the immutable generation record.
8. Atomically update redundant `AW_BOOTSTATE` records to select the new generation as trial-only.
9. Reboot.
10. Run kernel, storage, security, input, audio, accessibility, speech, and recovery-path health checks.
11. Mark the generation successful only after all mandatory checks pass.
12. If trial attempts are exhausted, return automatically to the most recent successful generation.

A power loss before boot-state commit leaves the old generation selected. A power loss after boot-state commit leaves both generations intact and allows retry/rollback.

## Boot-state record

`AW_BOOTSTATE` should use at least two independently checksummed/authenticated records with monotonically increasing sequence numbers. Each record should identify:

- selected generation digest/identifier;
- previous successful generation;
- trial flag;
- tries remaining;
- successful flag;
- rollback floor/index;
- format version;
- key-policy version.

GPT attributes are discovery metadata only and are not the authoritative slot/generation state.

## Mutable state

`AW_STATE` is encrypted at rest. Ordinary user data, downloads, caches, logs, temporary files, and application data are non-executable by default.

Third-party executable packages stored on mutable media must pass a verified package path before a process can be created from them. The process loader should consume authenticated executable capabilities rather than arbitrary writable filesystem paths.

## Migration path

### Phase 0 — current bootstrap

Physical A/B system images, independent recovery, encrypted state. Keep this because it is simple to inspect, boot, and recover while the storage stack is young.

### Phase 1 — semantic generation model

Implement `aw-generation`, structural invariants, accessibility health requirements, rollback floors, deterministic tests, and fuzz/property testing.

### Phase 2 — authenticated object format

Define versioned canonical serialization, explicit digest algorithm, object size limits, signed manifests, key roles, expiration/freeze policy, and offline root-key rotation.

### Phase 3 — append-only store

Implement crash-safe object admission, durable commits, reference indexing, corruption detection, and read-only executable mappings from verified objects.

### Phase 4 — multi-generation boot

Boot directly from immutable generation manifests. Keep multiple successful generations and trial rollback. Physical A/B becomes an installer/recovery fallback rather than the normal update mechanism.

### Phase 5 — garbage collection and delta transport

Add reachability-based collection and network delta/chunk transport without changing the trust model: reconstructed objects must still match their complete authenticated digest before admission.

## Non-goals for the first implementation

- no deduplicating writable filesystem in the trusted boot path;
- no Virtual-A/B/COW dependency before crash consistency is proven;
- no executing directly from unverified downloads;
- no mutable system generation;
- no single server key with authority to replace root trust;
- no marking a generation successful merely because a graphical desktop appeared.
