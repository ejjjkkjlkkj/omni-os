# Coverage Verification

Verified on commit `c0be738f607b9c682570b820074a841f42174914`.

## Machine-checked coverage

The repository CI verified:

- 95 master domains;
- 22 required domain groups;
- 16 mandatory assurance axes;
- 47 first-class accessibility sources;
- 21 publication / Web / overlay accessibility layers backed by sources;
- 23 full-stack accessibility layers, from L16 through L-6, backed by sources;
- no accessibility source accepted as a name-only placeholder;
- 21 publication / Web / overlay security layers;
- 23 deep security stack layers;
- language/configuration coverage checks;
- pinned third-party GitHub Actions;
- no obvious committed private-key blocks.

The security-intelligence refresh generated **43,070 normalized records from 130 configured sources**.

## Evidence runs

- Security + Accessibility Baseline: run `36353560572` — success.
- Update Threat Intelligence: run `36353560608` — success.

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

## Master-domain rule

Master-domain coverage additionally requires every domain to resolve to concrete source IDs and every unknown/emerging technology to enter the explicit coverage-gap path before merge.
