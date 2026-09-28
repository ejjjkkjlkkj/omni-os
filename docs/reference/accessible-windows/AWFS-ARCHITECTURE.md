# AWFS storage architecture

Status: design baseline. This document defines the target storage model for Accessible Windows; it is not yet a production on-disk format.

## Decision

Accessible Windows will not use one general-purpose writable filesystem for everything. The target storage stack deliberately separates two trust domains:

1. **AWStoreFS** — immutable, content-addressed and cryptographically verified storage for OS generations, recovery images, built-in components and verified application packages.
2. **AWStateFS** — encrypted, transactional copy-on-write storage for user data, settings, application state, logs and other mutable machine state.

Both formats may share common block primitives, validation code and tooling, but they have different security and performance policies. This keeps the boot path small and auditable while avoiding the complexity of putting snapshots, mutable state, package verification and recovery into a single filesystem.

## What we keep from existing designs

The design is clean-room and does not copy another on-disk format. We adopt properties that have demonstrated value:

- **OpenZFS:** end-to-end checksums, parent-authenticated block pointers, copy-on-write roots, scrub and repair from redundant copies.
- **Btrfs:** inexpensive snapshots/reflinks and checksumming of data and metadata, while explicitly avoiding RAID5/6-style filesystem parity in the first design.
- **XFS:** self-describing metadata with identity, owner, location and integrity validation before metadata is trusted.
- **APFS:** transactional copy-on-write metadata, snapshots, clones, volume separation and encryption as a first-class concern.
- **ReFS:** online scrub/repair ideas and block-clone semantics, without relying on a separate Storage Spaces implementation.
- **F2FS/Fxfs:** flash-friendly sequential mutation patterns, delayed reuse after deallocation, logical journaling and low write amplification.
- **EROFS / verity-style systems:** immutable system images, reproducible golden content and verification in the read path.

## What we deliberately do not copy

- No filesystem-integrated RAID5/6 or erasure coding in v1. Device redundancy belongs below the filesystem until the single-device format, repair model and crash semantics are mature.
- No synchronous global deduplication table. Content addressing naturally deduplicates AWStoreFS objects; AWStateFS uses reflinks/clones only when explicitly requested.
- No writable system root.
- No executable user-data volume by default.
- No in-place metadata overwrite.
- No `nocow` escape hatch in v1.
- No mount-time best-effort recovery that silently guesses which damaged metadata is correct. Ambiguous critical metadata fails closed into recovery.

## AWStoreFS

AWStoreFS is optimized for immutable executable content.

Properties:

- objects are addressed by a cryptographic digest;
- object contents never change after publication;
- generation manifests reference object digests, not mutable paths;
- directory/metadata images are reproducible;
- all executable content is verified before it becomes executable;
- reads are verified against Merkle roots or equivalent parent-authenticated digests;
- identical objects are stored once without maintaining a large synchronous dedup table;
- compression is allowed per immutable object/chunk because it cannot create mutable-state recovery ambiguity;
- garbage collection only removes objects unreachable from every retained generation, recovery root, application generation or rollback pin;
- a power loss during update cannot mutate the currently booted generation.

The initial physical A/B implementation remains a bootstrap. AWStoreFS is the long-term target beneath the multi-generation model in `aw-generation`.

## AWStateFS

AWStateFS is optimized for mutable user and machine state.

### Transaction model

Metadata and normal file data use copy-on-write. A transaction follows this order:

1. allocate fresh blocks;
2. write data extents;
3. write new metadata nodes that reference already-written children;
4. flush the device barrier required by the block layer;
5. write a new root commit record into the next superblock/checkpoint slot;
6. flush that commit;
7. only then make old unreferenced extents eligible for later reuse/TRIM.

A crash before step 5 leaves the previous root authoritative. A crash after a completely persisted step 5 selects the new root. Blocks deallocated by an uncommitted transaction are never reused early.

### Root/checkpoint ring

AWStateFS reserves multiple physically separated checkpoint records rather than one mutable superblock. Each valid record includes at least:

- format/version;
- monotonically increasing transaction sequence;
- volume identity;
- root block pointer;
- root digest;
- previous committed root digest;
- feature flags;
- encryption/key-generation metadata references;
- checksum/authentication field for the checkpoint itself.

Mount chooses the newest structurally valid, cryptographically valid and internally consistent checkpoint. Equal sequence numbers with different roots are treated as corruption, not broken ties by guesswork.

### Metadata blocks

Every metadata block is self-describing and has bounded parsing. Its canonical header includes at least:

- magic and format version;
- block kind;
- header and payload lengths;
- volume/object owner;
- logical identifier;
- transaction generation;
- physical/logical location expectation;
- feature flags;
- child/content digest information.

Parsers validate integer arithmetic, bounds, type, owner, location, version and digest before exposing fields to higher layers.

### Integrity

Metadata integrity is mandatory. File-data integrity is also mandatory by default. Digests are stored in parent metadata rather than allowing a corrupted block to authenticate itself. When redundant storage below the filesystem can provide another copy, scrub may repair a bad extent only after verifying the alternate copy.

A checksum is corruption detection, not authenticity. Encrypted/authenticated AWStateFS extents use an AEAD construction and explicit key generations; immutable AWStoreFS trust roots are authenticated by signed generation manifests.

### Encryption

Encryption is part of the format contract rather than an optional afterthought. The design supports:

- per-volume root keys;
- separately wrapped per-file or per-object keys;
- explicit key-generation identifiers;
- TPM-sealed wrapping when available, with a documented non-TPM fallback;
- metadata confidentiality for user-sensitive names and attributes;
- cryptographic erasure by destroying wrapped keys where appropriate.

Exact algorithms are intentionally not frozen until the crypto layer and threat model are reviewed; the on-disk format must permit algorithm agility without accepting downgrade.

### Snapshots and clones

A snapshot is an immutable reference to a committed root. File/directory clones share immutable extents and use reference accounting. Snapshot deletion and clone modification must never require rewriting the referenced old root.

### Flash/NVMe policy

- 4 KiB minimum logical block baseline, with device geometry discovered separately;
- batching/coalescing of small metadata mutations;
- delayed discard/TRIM after durable deallocation;
- no gratuitous synchronous dedup lookup on every write;
- background compaction only for structures that benefit from it and with bounded write-amplification accounting;
- explicit latency budgets so accessibility-critical audio/input state is not blocked behind long storage maintenance operations.

## Accessibility is a storage invariant

Storage recovery cannot assume sight. Recovery tooling must expose a stable semantic interface consumable by speech and braille before the graphical desktop is available.

The system must preserve at least one bootable generation whose minimum accessibility path is known-good. A new generation is not allowed to become the sole retained generation until input, audio, accessibility broker and speech health checks have passed.

Filesystem repair tools must produce machine-readable structured diagnostics in addition to visual text so recovery UI and screen readers can announce exact damage and available actions.

## Failure policy

Critical metadata uses fail-closed rules:

- malformed length, arithmetic overflow or out-of-range block => reject;
- checksum/authentication failure => reject the block;
- conflicting newest checkpoints => recovery, never arbitrary selection;
- unsupported required feature => refuse read-write mount;
- executable object without verified AWStoreFS provenance => refuse execution;
- encrypted extent without an authenticated key generation => refuse plaintext exposure.

Read-only salvage may expose independently verified files, but it must never rewrite metadata automatically while evidence is ambiguous.

## Implementation stages

1. `aw-fs-core`: safe `no_std` canonical primitives, checkpoint selection and bounded validation only; no disk writes.
2. AWStoreFS object/manifest reader with deterministic encoding and digest verification.
3. Read-only AWStateFS parser plus image builder and corruption/property tests.
4. Transaction writer with crash-injection tests at every persistence boundary.
5. Snapshots, clones, quotas and online scrub.
6. Encryption/key hierarchy and TPM integration after independent crypto review.
7. Real NVMe/SATA fault-injection, power-loss and long-duration fuzz testing.

No filesystem is called production-ready until image fuzzing, model/property tests, simulated torn writes, power-loss cut points and real hardware tests all pass.
