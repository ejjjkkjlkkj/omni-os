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

The repository does not yet contain a project license. A compatible licensing policy must be selected before accepting substantial third-party contributions or importing any external source code.
