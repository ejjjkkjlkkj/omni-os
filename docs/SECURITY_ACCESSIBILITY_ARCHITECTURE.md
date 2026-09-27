# OMNI Security + Accessibility Architecture

OMNI treats cybersecurity and accessibility as co-equal system properties.

## Non-negotiable rule

No security control is complete until a blind user can understand it, operate it, recover from it, and verify its result without sight.

Likewise, no accessibility path is trusted until it has defined confidentiality, integrity, authenticity, authorization, logging, and recovery behavior.

## Dual assurance model

Every feature must satisfy both columns.

| Cybersecurity | Accessibility |
|---|---|
| Confidentiality | Secret-safe speech/braille |
| Integrity | Authentic spoken/braille state |
| Availability | Accessible offline/recovery path |
| Authenticity | Trusted accessibility path |
| Authorization | Keyboard/braille operable approval |
| Accountability | Accessible event explanation |
| Resilience | Accessible degraded mode |
| Privacy | Redaction of sensitive semantic output |
| Recovery | Spoken/braille recovery workflow |
| Evidence | Human-readable + machine-readable proof |

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

Security and accessibility must persist across every transition.

## Trusted accessibility path

Security-sensitive prompts must not be spoofable by ordinary applications.

Required properties:

- trusted-input gesture reserved to the security path;
- security-mode speech cue;
- braille security prefix/state;
- semantic role/name/value/state/action;
- secret-field suppression;
- no plaintext remote mirroring of secret fields;
- no logging of passwords, PINs, recovery secrets or private keys;
- explicit timeout and cancellation behavior;
- accessible confirmation of success/failure/recovery.

## Fail-secure, remain-accessible

A network, VPN, GitHub, DNS, TPM, audio, or update failure must not silently remove local accessibility.

At minimum, OMNI must preserve an accessible local recovery path with deterministic error reporting.

## Evidence contract

Every security event should be representable as:

```text
event_id
severity
component
category
reason
action_taken
user_options
privacy_class
accessibility_rendering
evidence_reference
```

The same event must be renderable through visual text, local speech, braille, keyboard navigation, and structured logs when policy allows.

## Definition of done

A feature is not DONE unless:

1. its threat model is documented;
2. privileges and secrets are identified;
3. failure and recovery behavior are defined;
4. accessible interaction is defined;
5. secret-safe speech/braille behavior is defined;
6. automated tests exist where feasible;
7. evidence proving the result is produced.
