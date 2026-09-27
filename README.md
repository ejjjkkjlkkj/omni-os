# OMNI Security + Accessibility

OMNI is designed around **two co-equal pillars**:

1. **Cybersecurity**
2. **Accessibility**

Neither pillar may weaken the other.

A security control is incomplete if a blind user cannot understand, operate, verify, and recover from it without sight.

An accessibility feature is incomplete if it exposes secrets, weakens authorization, breaks integrity, or creates a spoofable trusted path.

## Continuous chain

```text
Hardware
-> UEFI
-> PreEnvironment
-> Recovery
-> Loader
-> Kernel
-> System
-> OS
```

## Cybersecurity planes

- Confidentiality
- Integrity
- Availability
- Authenticity
- Identity and authorization
- Cryptography and key management
- Zero Trust
- Network security
- IDS / IPS
- VPN
- Privacy and metadata protection
- TPM / attestation
- Supply-chain security
- Hardware trust
- Recovery and resilience
- Evidence and forensics
- Maximum privilege with bounded elevation

## Accessibility planes

- Semantic UI
- Keyboard-only control
- Local speech
- Braille
- Trusted accessibility path
- Secret-safe input/output
- Accessible security prompts
- Accessible degraded mode
- Accessible offline recovery
- Accessible evidence and diagnostics
- Secure remote semantic access

## Core rule

```text
CYBERSECURITY <-> ACCESSIBILITY
```

Every OMNI requirement must define both its security property and its accessibility property.

See:

- `SECURITY.md`
- `docs/SECURITY_ACCESSIBILITY_ARCHITECTURE.md`
- `docs/MAXIMUM_PRIVILEGE_MODEL.md`
- `docs/PHYSICAL_OPERATOR_SESSION.md`
- `docs/KERNEL_USER_BOUNDARY.md`
- `docs/PROTECTED_READ_ONLY_MODEL.md`
- `requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv`
