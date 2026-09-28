# OMNI Accessibility Across Surface / Deep / Dark Web and Silicon

Accessibility is not a separate feature beside threat intelligence.

It is a required assurance axis across the same OMNI stack used for Surface Web,
Deep Web, Dark Web, overlays, network, kernel, firmware, hardware and silicon.

## Core invariant

```text
NO SECURITY LAYER IS COMPLETE
WITHOUT AN ACCESSIBLE OBSERVATION / CONTROL / RECOVERY PATH.
```

For a blind operator, every layer must answer:

- What state exists?
- What changed?
- Is it trusted?
- What action is available?
- What action is dangerous?
- What is secret and therefore redacted?
- What happens if speech fails?
- What happens if braille fails?
- What happens if the normal UI is compromised?
- What evidence proves the state?

## Web and overlay coverage

The accessibility plane applies equally to:

- Surface Web;
- non-indexed Deep Web;
- authenticated portals;
- closed communities;
- Tor onion services;
- I2P;
- ZeroNet;
- IPFS/libp2p;
- Hyphanet;
- GNUnet;
- Yggdrasil;
- cjdns/Hyperboria;
- Nym/mixnets;
- mesh/other overlays;
- CTI reports;
- local SecLab imports.

These are source/provenance classes, not accessibility exceptions.

### Required behavior

When OMNI presents intelligence from any of these layers, the normalized interface
must expose at minimum:

- semantic entity type;
- name/alias;
- source/provenance layer;
- confidence or source wording when available;
- date/time;
- technical indicators;
- relationship to actor/campaign/tool/malware/vulnerability;
- risk/authorization context;
- keyboard navigation;
- speech output;
- braille output;
- machine-readable JSON;
- secret/private-data redaction.

Raw inaccessible web pages are not the canonical OMNI user interface. The local
normalized database is.

## Full-stack accessibility chain

```text
L16  Governance
     accessibility is a security requirement, not optional UX

L15  Human / operator
     trusted attention, keyboard-only control, spoken/braille confirmation

L14  Identity
     accessible login, MFA/passkeys, recovery, secret-safe prompts

L13  Application
     semantic UI, focus, role/name/value/state/action

L12  Data / content
     accessible structured rendering of documents, logs, CTI and errors

L11  Language / runtime
     crashes/exceptions/debug output remain machine-readable and accessible

L10  Build / CI / packages
     failures and provenance exposed as structured text, not visual-only status

L9   Sandbox / IPC / services
     accessibility broker/capabilities are explicit and least-privileged

L8   Naming / discovery
     DNS/DHT/IPNS/name-resolution state is queryable and announced

L7   Crypto / session
     certificate/key/trust decisions are accessible and secret-safe

L6   Overlay / privacy / P2P
     Tor/I2P/ZeroNet/IPFS/etc. connection/trust state is accessible

L5   Transport
     connection state and failure reason exposed semantically

L4   Network / routing
     route/tunnel/firewall state available through accessible diagnostics

L3   Link / radio
     Wi-Fi/Bluetooth/NFC/cellular state accessible without pointer-only UI

L2   Device / driver / DMA
     audio, braille, keyboard and HID device health exposed with bounded evidence

L1   Kernel / hypervisor
     minimum emergency accessibility path survives user-space failure

L0   Firmware / boot
     pre-OS keyboard/text/audio/braille path where supported

L-1  Component authentication
     SPDM/PCIe/device trust result can be rendered by speech/braille

L-2  Root of trust
     TPM/DICE/Secure Element attestation state is inspectable without exposing keys

L-3  Silicon security state
     lifecycle/debug/entropy/RoT state has machine-readable evidence

L-4  Manufacturing
     provenance/counterfeit/tamper evidence can be surfaced accessibly

L-5  Physical
     hardware-fault/tamper state must not be represented by LED/color only

L-6  Supply-chain lifecycle
     build/manufacture/provision/update/RMA/decommission evidence remains accessible
```

## Screen reader architecture

The full screen reader remains outside the kernel.

```text
application semantics
        ↓
OMNI semantic accessibility broker
        ↓
screen reader
        ↓
speech / braille / keyboard interaction
```

The kernel and lower layers provide only mechanisms that must survive or be
non-bypassable:

- trusted input path;
- device ownership/isolation;
- emergency text/event channel;
- audio/HID/braille transport primitives;
- protected security-state events;
- panic/recovery path;
- IOMMU/DMA protection for accessibility devices.

## Pre-OS chain

UEFI defines text console input/output protocols intended to exchange text with
the system user during Boot Services. OMNI treats those protocols as a minimum
semantic transport and layers its own accessibility behavior above them.

Target chain:

```text
Boot ROM / firmware
    ↓
UEFI ConsoleIn / ConsoleOut / Serial / HID
    ↓
OMNI pre-OS semantic events
    ↓
minimal TTS / braille / keyboard navigation
    ↓
Recovery / Loader
    ↓
Kernel emergency accessibility
    ↓
full user-space screen reader
```

## Trusted accessibility path

A hostile web page, malware, compromised app or even ordinary untrusted application
must not be able to impersonate a trusted OMNI security announcement.

Security-critical events therefore need:

- source identity;
- security-domain identifier;
- trusted speech prefix/cue;
- trusted braille prefix/state;
- bounded semantic payload;
- secret redaction;
- anti-spoofing policy;
- evidence reference.

## Failure rule

Accessibility failure is a security and availability failure.

Examples:

- screen reader process crashes -> restart + emergency channel;
- audio stack fails -> braille/text/serial fallback;
- braille device fails -> speech/text fallback;
- user-space compromised -> trusted security channel remains distinguishable;
- network/overlay fails -> local database and recovery remain usable;
- firmware graphics inaccessible -> text/serial/keyboard path remains primary fallback.

## Evidence

Each layer must define an accessibility proof, not merely state that it is
"accessible".

Examples:

- deterministic keyboard navigation test;
- semantic role/name/value/state fixture;
- spoken output transcript hash;
- braille output fixture;
- device attach/detach event;
- recovery interaction test;
- secret-redaction test;
- trusted-path spoofing negative test.
