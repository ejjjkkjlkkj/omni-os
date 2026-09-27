# Coverage Verification

Verified on commit `2a6d1a53a5f5249e075edf5f5732c3f84b532986`.

## Machine-checked coverage

The repository CI verified:

- 46 first-class accessibility sources;
- 21 publication / Web / overlay accessibility layers backed by sources;
- 23 full-stack accessibility layers, from L16 through L-6, backed by sources;
- no accessibility source accepted as a name-only placeholder;
- 21 publication / Web / overlay security layers;
- 23 deep security stack layers;
- language/configuration coverage checks;
- pinned third-party GitHub Actions;
- no obvious committed private-key blocks.

The security-intelligence refresh generated **43,042 normalized records from 102 configured sources**.

## Evidence runs

- Security + Accessibility Baseline: run `36353042447` — success.
- Security + Accessibility Baseline: run `36353039323` — success.
- Update Threat Intelligence: run `36353039390` — success.

## Coverage rule

A term in documentation is not considered coverage.

Coverage requires all applicable machine-readable relationships:

```text
source
  -> domain
  -> publication/provenance layer
  -> stack layer
  -> proof/evidence type
  -> validation rule
  -> CI result
```

For accessibility, this applies from public Web and screen-reader standards through
Deep/Dark/overlay provenance, application semantics, accessibility APIs, reader/TTS/
braille paths, kernel/driver/firmware state, roots of trust, silicon, manufacturing,
physical state and lifecycle.

## Unknowns

Unknown technologies are coverage gaps, not implicit safety.

A future source, overlay, language, runtime, accessibility API, device, firmware or
hardware trust mechanism must be mapped and validated before the baseline can claim
coverage.
