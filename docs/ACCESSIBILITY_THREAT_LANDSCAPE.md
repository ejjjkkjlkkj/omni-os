# OMNI Accessibility Threat Landscape

Accessibility and screen-reader security are first-class intelligence domains.

OMNI models the same continuum used for cybersecurity:

```text
Surface / white web
  -> Deep / authenticated web
  -> closed communities
  -> Tor / I2P / ZeroNet / IPFS / other overlays
  -> application/runtime
  -> OS/kernel/hypervisor
  -> firmware / UEFI
  -> component authentication
  -> TPM / RoT
  -> silicon
  -> manufacturing / physical lifecycle
```

Dark-web or overlay origin is provenance, not proof of maliciousness.

## Accessibility-specific defensive intelligence

OMNI tracks, when publicly documented or observed in authorized SecLab imports:

- screen-reader vulnerabilities;
- accessibility API abuse;
- malicious accessibility services;
- semantic-tree poisoning or deceptive accessibility metadata;
- focus hijacking;
- trusted speech/braille spoofing;
- keystroke/input capture through accessibility privileges;
- unauthorized input injection;
- secret leakage through speech, braille, logs or remote accessibility;
- screen-reader add-on/plugin supply-chain compromise;
- TTS engine/model/voice supply-chain compromise;
- braille driver/device vulnerabilities;
- HID spoofing or malicious assistive peripherals;
- audio path denial/tampering;
- accessibility denial-of-service;
- inaccessible security/recovery prompts;
- firmware/pre-OS accessibility failures;
- device firmware replacement;
- DMA/IOMMU failures affecting audio/HID/braille;
- hardware-root/attestation failures affecting trusted accessibility;
- silicon/debug/fault-injection conditions that can corrupt the trusted path.

## White / public Web sources

Examples include:

- W3C WAI-ARIA, Core AAM, Accessible Name and Description Computation, WCAG;
- Microsoft UI Automation;
- Apple accessibility / VoiceOver documentation;
- Android AccessibilityService / TalkBack;
- Linux AT-SPI / Orca;
- NVDA;
- BRLTTY;
- UEFI console protocols;
- USB HID specifications;
- public CVE/CWE/CAPEC/ATT&CK/D3FEND records affecting these components.

## Deep / Dark / overlay sources

OMNI accepts normalized **security metadata** from authorized collectors on:

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
- other overlays.

These imports may record tool/malware/campaign names, public hashes/IOCs, exploit
references, affected accessibility components, techniques, source provenance and
confidence.

Raw stolen credentials, private communications and private-person dossiers are
not stored.

## Full-stack accessibility attack surface

| Layer | Accessibility attack surface |
|---|---|
| L16 governance | disabling accessibility policy, inaccessible authorization |
| L15 human | social engineering through spoken/braille trust cues |
| L14 identity | inaccessible MFA/recovery, secret prompt leakage |
| L13 app | deceptive labels/roles, focus spoofing, inaccessible controls |
| L12 data | malicious documents/content targeting AT parsers |
| L11 runtime | plugin/add-on/native-extension compromise |
| L10 supply chain | poisoned screen reader/TTS/braille packages or CI |
| L9 IPC/services | accessibility broker abuse, over-broad AT privileges |
| L8 naming | misleading source/name resolution presented to operator |
| L7 crypto | inaccessible certificate/key warnings, trust spoofing |
| L6 overlays | malicious content/tools targeting AT through Tor/I2P/P2P |
| L5 transport | disrupted remote accessibility channel |
| L4 routing | route/tunnel state hidden from nonvisual operator |
| L3 link/radio | malicious Bluetooth/USB/Wi-Fi assistive device path |
| L2 device/DMA | HID/audio/braille driver bugs, DMA misuse |
| L1 kernel/hypervisor | event/input/output mediation compromise |
| L0 firmware | inaccessible or spoofed pre-OS/recovery state |
| L-1 component auth | untrusted audio/HID/braille firmware/device |
| L-2 RoT | fake/invalid attestation of accessibility components |
| L-3 silicon | debug/fuse/entropy/security-state compromise |
| L-4 manufacturing | counterfeit/tampered controller/codec/device |
| L-5 physical | interposer, fault injection, malicious replacement |
| L-6 lifecycle | compromised provisioning/update/RMA/decommission |

## Requirement

Accessibility is complete only when both are known:

```text
HOW THE USER ACCESSES THE STATE
and
HOW AN ADVERSARY CAN ATTACK THAT ACCESS PATH
```
