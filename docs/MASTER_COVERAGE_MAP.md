# OMNI Master Coverage Map

This is the top-level map for the OMNI security and accessibility knowledge base.

## What "complete" means here

OMNI does not claim that a finite list can prove that no future technology or threat exists.
Instead, completeness means:

1. all currently required domains are explicitly mapped;
2. every domain points to concrete source IDs;
3. every source ID resolves in the source registry;
4. security and accessibility share the same Web/overlay and deep-stack model;
5. unknown technologies become explicit coverage gaps instead of being silently accepted;
6. CI fails when a required group, source, layer, or anchor domain disappears.

The canonical machine-readable map is:

`data/taxonomy/master-coverage.json`

The canonical validator is:

`scripts/check_master_coverage.py`

## Current verified scope

- 95 master domains
- 22 required groups
- 16 mandatory assurance axes
- 21 Surface / Deep / Dark / overlay source planes
- 23 security/accessibility layers from L16 through L-6
- 130 configured source IDs

## Required assurance axes

Every domain is evaluated against:

- assets;
- trust boundaries;
- threats;
- attack surface;
- protections;
- detections;
- response;
- recovery;
- evidence;
- reference state;
- attestation or verification;
- privacy;
- accessibility;
- privilege model;
- update / rollback;
- source provenance.

## Domain groups

The map covers governance, engineering, human factors, operations, threats,
identity, cryptography, privacy, data, software, platform, cloud, firmware,
network, verticals, emerging technology, supply chain, resilience, assurance,
hardware, accessibility and meta/unknown coverage.

## Representative domains

The full JSON is authoritative. It includes, among others:

- governance, risk, policy and authorization;
- threat intelligence, actors, campaigns and TTPs;
- vulnerability management, CWE/CAPEC and exploit prioritization;
- red team, blue team, SOC, incident response and forensics;
- malware, ransomware, botnets and commercial intrusion tooling;
- identity proofing, authentication, WebAuthn, privilege and zero trust;
- classical cryptography, PQC and crypto agility;
- privacy, metadata protection and anonymity;
- memory safety, languages, runtimes, JITs and virtual machines;
- Web/API, browser, desktop, server and mobile;
- cloud, containers, Kubernetes, edge and serverless;
- hypervisors and confidential computing;
- kernel, drivers, DMA/IOMMU and devices;
- UEFI, BMC, EC, ACPI, option ROM and device firmware;
- routing, transport, DNS, naming, wireless and RF;
- Surface Web, Deep Web, Tor, I2P, ZeroNet, IPFS, Hyphanet, GNUnet,
  Namecoin, Yggdrasil, cjdns/Hyperboria, Nym, libp2p and other overlays;
- telecom/5G, IoT, OT/ICS, automotive, medical, maritime, rail, aviation,
  energy, payments, space/satellite and critical infrastructure;
- AI/ML/models/agents, robotics/drones, XR and emerging systems;
- CI/CD, package registries, SBOM, provenance and artifact signing;
- supply chain, manufacturing, logistics, RMA and decommission;
- root of trust, TPM, DICE, HSM, Secure Element, SPDM, RIM and attestation;
- CPU/GPU/NPU/FPGA, silicon, microcode, fuses, debug/JTAG and entropy;
- physical tamper, side channel, fault injection and environmental threats;
- screen readers, accessibility APIs, semantics, TTS, audio, braille, HID,
  haptics, keyboard/switch/voice control, multimodal disability support,
  pre-OS accessibility and accessibility down to hardware/silicon;
- safety, real-time behavior, fail-safe operation and recovery;
- formal methods, fuzzing, property testing, chaos testing and evidence;
- an explicit unknown/emerging/unclassified domain.

## Lifecycle

Coverage spans research, design, development, build, test, manufacture,
integration, provisioning, deployment, operation, monitoring, update, incident,
recovery, repair/RMA, decommission and destruction.

## Unknown rule

A new technology, protocol, device, language, overlay, actor class,
accessibility mechanism or trust primitive that cannot be mapped MUST create a
coverage-gap record.

```text
UNKNOWN
  -> COVERAGE GAP
  -> SOURCE RESEARCH
  -> DOMAIN/LAYER MAPPING
  -> THREAT + DEFENSE + EVIDENCE
  -> CI VALIDATION
  -> COVERED
```

Unknown never means safe.
