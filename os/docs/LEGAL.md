# Source and clean-room policy

## Purpose

Accessible Windows must be safe to develop publicly and must not depend on unauthorized proprietary source code.

## Allowed inputs

Subject to their individual license terms:

- public standards and specifications;
- vendor hardware documentation;
- Microsoft Open Specifications;
- official SDK/WDK documentation and samples where redistribution permits it;
- appropriately licensed open-source projects;
- observable API/ABI behavior obtained through lawful interoperability testing;
- original implementation and tests written for this project.

## Prohibited inputs

Do not copy, translate, adapt or import code from:

- leaked Windows source trees;
- unauthorized mirrors of proprietary Microsoft code;
- proprietary binaries decompiled into source for direct incorporation;
- source whose license is incompatible or unknown.

A public repository containing code does not automatically make that code open source.

## Compatibility work

Compatibility tests should describe externally observable behavior and expected results. Implementations should be written from documented contracts and independently derived tests.

## Licensing status

The project's own code is licensed under the Zero-Clause BSD license (0BSD): see [`LICENSE`](../../LICENSE) at the root of omni-os. Third-party components keep their own licenses; imported external source must be license-compatible and recorded before it is accepted.
