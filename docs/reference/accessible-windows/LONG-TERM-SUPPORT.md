# Long-term support policy

## Support target

Accessible Windows major generations are designed for a **minimum ten-year support horizon**. This is an engineering compatibility target, not a promise that every hardware vendor will continue shipping firmware or replacement parts for ten years.

The reference point is long-lived operating-system servicing such as Windows IoT Enterprise LTSC, while avoiding dependence on proprietary Windows internals.

Microsoft lifecycle references:

- Windows 11 IoT Enterprise LTSC 2024: support through October 2034.
  - https://learn.microsoft.com/lifecycle/products/windows-11-iot-enterprise-ltsc-2024
- Windows 11 Enterprise LTSC 2024 has a shorter five-year lifecycle, so Accessible Windows deliberately adopts the longer ten-year target rather than tying policy to one Windows edition.
  - https://learn.microsoft.com/lifecycle/products/windows-11-enterprise-ltsc-2024

## Compatibility principles

### Stable contracts first

Public kernel/user ABI, recovery events, persistent boot state, system-store manifests and update envelopes must carry explicit versioning.

Within one supported major generation:

- compatible additions increment a minor version;
- incompatible changes require a new major version;
- readers reject unknown incompatible major versions;
- append-only identifiers are preferred for diagnostics and protocol enums;
- removed behavior remains parseable long enough for migration/recovery tooling;
- migration code is tested from every still-supported format generation.

### Recovery must outlive normal userspace

The Recovery Core and its external recovery media are compatibility anchors.

A recovery image released late in a support window should be able to:

- diagnose boot-state records from earlier supported releases;
- identify old supported generations;
- export diagnostics without booting the installed desktop;
- migrate boot metadata only through explicit, tested transformations;
- reinstall a still-supported signed generation;
- preserve user state where policy permits;
- explain unsupported/incompatible states through stable speech/braille diagnostics.

### No flag-day cryptographic migrations

Hash/signature/key changes require overlap periods.

A transition must define:

1. old algorithm still accepted for existing supported generations;
2. new algorithm accepted and produced by current tooling;
3. signed migration policy defining the anti-rollback floor;
4. recovery media capable of validating both during the overlap;
5. retirement only after every supported upgrade path has crossed the migration boundary.

Algorithm identifiers must be explicit. Do not infer algorithms from digest length or key shape.

### Atomic update lineage

Each update creates a complete generation. Core executable content is never patched in place.

At all times the machine keeps enough authenticated information to choose between:

- current known-good generation;
- pending/trial generation;
- Recovery Core;
- signed external recovery media.

Power loss must never require reinstall merely because an update was in progress.

### Long-lived hardware strategy

Prefer standards before vendor-specific paths:

- UEFI x64;
- ACPI;
- PCI/PCIe class discovery;
- NVMe;
- AHCI/SATA where applicable;
- xHCI/USB HID;
- HDA baseline;
- GOP framebuffer fallback.

Vendor-specific behavior lives behind isolated drivers/quirks and must not leak into stable generic contracts.

Dropping a previously supported hardware class inside a supported major generation requires explicit justification, migration/recovery analysis and release documentation. A newer optimized driver must not remove the conservative fallback unless that fallback is demonstrably unsafe.

### Toolchain independence

The on-disk formats and public protocol semantics must not depend on one Rust compiler version, crate version or build-host operating system.

CI therefore needs to retain:

- committed dependency locks;
- reproducible-format fixtures;
- parser tests using old-format golden images;
- future Rust toolchain checks;
- build provenance/SBOM;
- migration tests independent of the compiler that created the original image.

### Accessibility is a compatibility contract

Accessibility behavior used for boot, recovery and installation is part of the long-term interface.

Within a supported major generation:

- diagnostic codes remain stable;
- critical keyboard navigation remains deterministic;
- an existing safe key sequence must never become destructive;
- no destructive action can become timeout-driven;
- speech/braille event semantics remain versioned and testable;
- recovery must remain operable without pointer input;
- visual-only success never counts as accessible success.

An update that boots graphically but breaks nonvisual recovery is a failed update and must not become known-good.

## Ten-year validation matrix

The project should maintain fixtures representing old supported releases and continuously test current code against them.

Minimum matrix:

- boot-state major/minor parsing;
- generation manifest parsing;
- authenticated object verification;
- rollback-floor migration;
- recovery diagnostic decoding;
- keyboard recovery semantics;
- update envelope verification;
- cryptographic algorithm transition fixtures;
- old known-good -> current trial -> rollback;
- current recovery media -> old supported installation;
- old recovery media behavior when encountering a newer incompatible generation (must fail clearly, not corrupt state).

## Release policy

A release cannot claim long-term support readiness until:

- persistent formats have explicit compatibility rules;
- migrations are power-loss safe;
- rollback remains available across update boundaries;
- recovery tooling covers every still-supported persistent format;
- accessibility recovery paths are included in compatibility tests;
- physical AMD and Intel x64 validation covers representative old and current hardware;
- unsupported hardware/format states fail with actionable nonvisual diagnostics rather than hanging or guessing.

## Design rule

When choosing between a clever optimization and a simpler mechanism that is easier to recover, migrate, test and explain ten years later, prefer the simpler mechanism unless measurements prove the complexity is necessary.
