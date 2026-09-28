# Google Play / GMS edition

## Objective

Accessible Android supports a second build edition intended for environments where Google Mobile Services (GMS) may be used legitimately:

- `accessible-aosp`: open and redistributable Android build without proprietary Google applications;
- `accessible-gms`: Android build prepared for Google Play Store and Google Play services when an authorized GMS bundle is supplied by the builder.

The accessibility contract is identical for both editions. Speech, screen reader availability, keyboard setup and recovery must not depend on the Play Store.

## Licensing boundary

Google Mobile Services, including Google Play Store and Google Play services, are not part of AOSP. This repository therefore does not contain, mirror, scrape, download or redistribute proprietary Google APKs or privileged application bundles.

A GMS build requires the builder to provide the files separately under terms that authorize their use. The build tooling treats those files as private build inputs and prevents them from being committed accidentally.

Passing AOSP compatibility tests alone does not grant permission to redistribute GMS. A production device/distribution that ships Google applications must follow Google's compatibility, certification and licensing process.

## Private input contract

Set:

```bash
export GMS_BUNDLE_DIR=/absolute/path/to/authorized-gms-bundle
```

The directory must contain:

```text
LICENSE_ACCEPTED
gms-bundle.manifest
payload/
```

`LICENSE_ACCEPTED` is a local acknowledgement controlled by the builder. It is not a substitute for an actual Google license or other applicable authorization.

`gms-bundle.manifest` must contain SHA-256 entries in standard `sha256sum` format for every file used from `payload/`.

Example structure:

```text
authorized-gms-bundle/
├── LICENSE_ACCEPTED
├── gms-bundle.manifest
└── payload/
    └── ... authorized proprietary build inputs ...
```

The repository intentionally does not define or ship Google's proprietary payload layout.

## Build

Open edition:

```bash
./scripts/build-edition.sh accessible-aosp
```

GMS-ready edition:

```bash
GMS_BUNDLE_DIR=/secure/gms ./scripts/build-edition.sh accessible-gms
```

The GMS build first validates the private input and then selects the dedicated `accessible_android_x86_64_gms` Android product. Until that product is implemented by the Android 17 PC BSP, the build fails rather than silently producing an AOSP-only image.

## Required validation for the GMS edition

A release candidate must verify at runtime:

1. Android boot completion and persistent storage;
2. package manager operation;
3. Google Play Store package presence when legitimately supplied;
4. Google Play services package presence when legitimately supplied;
5. account/add-account UI accessibility;
6. Play Store keyboard and screen-reader navigation;
7. app install/update flow;
8. Play Protect/certification state reported accurately;
9. offline TTS and screen-reader recovery remain functional without Google services;
10. CTS and accessibility regression suites.

## Accessibility rule

Google services are optional from the accessibility architecture's point of view. The machine must never become unusable for a blind user because Google account setup, network access, Play Services, or the Play Store is unavailable.
