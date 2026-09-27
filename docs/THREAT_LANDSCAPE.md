# OMNI Threat Landscape

OMNI keeps a local, source-backed security knowledge base covering the full spectrum:
authorized research and defense, dual-use tooling, and documented hostile activity.

## Authorization line

The boundary is authorization, not skill level or the word "hacker".

```text
AUTHORIZED
  explicit permission + defined target + defined scope + bounded impact
---------------- OMNI AUTHORIZATION LINE ----------------
UNAUTHORIZED / OUT OF SCOPE
  no permission, ambiguous permission, exceeded scope, or unapproved impact
```

## Covered actor classes

Authorized/defensive:
security researchers, vulnerability researchers, bug bounty, pentesters, red teams,
blue teams, purple teams, SOC, CERT/CSIRT, incident response, forensics, reverse
engineering, malware analysis, threat hunting, detection engineering, fuzzing,
secure development, supply-chain review, hardware/firmware audit, cloud/mobile/OT
security and authorized adversary emulation.

Unauthorized/hostile:
state and state-sponsored actors, publicly documented military/intelligence cyber
activity, APT groups, cybercrime/eCrime, ransomware operators and affiliates,
initial-access brokers, botnets, credential theft, phishing, malware operators,
loaders, stealers, RATs, backdoors, C2 infrastructure, exploit kits, wipers,
extortion, commercial spyware abuse, exploit brokers, malicious insiders,
hacktivists and unattributed campaigns.

## Dual-use tools

Scanners, debuggers, disassemblers, fuzzers, exploit-development frameworks,
remote administration tools, packet tools, forensics tools, red-team frameworks,
malware-analysis tools and defensive tooling are represented. A tool is not
classified as malicious merely because it can be used offensively.

OMNI records capability, observed use, authorization context, source, aliases,
platforms, techniques, mitigations/detections and confidence.

## Web coverage

The same database accepts observations from:

- Surface Web / clear web;
- authenticated Deep Web sources used with authorization;
- closed communities and portals;
- Tor onion services;
- I2P services;
- other overlay/anonymity networks;
- CTI reporting about dark-web activity;
- local SecLab collection imports.

The data policy excludes raw personal data and secrets, not security tooling or
threat categories.

## Covered technical domains

Enterprise, Windows, Linux, macOS, Android, iOS, UEFI/firmware, kernels/drivers,
hardware/PCIe/DMA/IOMMU, identity, cryptography, network, Wi-Fi, Bluetooth, NFC,
web/API, cloud, containers/Kubernetes, CI/CD, package ecosystems, supply chain,
AI/ML/agents, virtualization/hypervisors, OT/ICS, IoT/embedded, storage,
databases, browsers, email/messaging, telecom, satellite/space and accessibility.

Generated data lives under `data/threat-intel/generated/`.
