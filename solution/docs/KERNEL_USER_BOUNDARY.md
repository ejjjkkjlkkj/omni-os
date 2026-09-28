# OMNI Kernel / User Boundary

## Core rule

OMNI follows a strict mechanism/policy split:

```text
KERNEL = enforce what userspace must not be allowed to bypass
USERSPACE = everything else
```

The kernel is not a general application platform.

If a function can safely live outside ring 0, it must live outside ring 0.

## Kernel responsibilities

OMNI Kernel owns only operations that require hardware authority or non-bypassable enforcement:

- virtual memory and page protection;
- process/thread isolation primitives;
- capability enforcement;
- interrupt and exception handling;
- scheduler core;
- CPU privilege transitions;
- IOMMU / DMA policy enforcement;
- device access mediation;
- kernel driver isolation boundary;
- secure handle/object ownership;
- boot handoff validation;
- protected kernel evidence primitives;
- trusted time/monotonic primitives where available;
- protected entropy interface;
- TPM / hardware-root mediation where direct privileged access is required;
- firmware/NVRAM access mediation;
- raw physical disk mediation;
- kernel panic / emergency recovery path;
- minimal trusted accessibility fallback required to report fatal kernel state.

## User-space responsibilities

Everything that does not require ring-0 authority belongs outside the kernel:

- full screen reader;
- normal TTS;
- braille translation;
- semantic accessibility tree;
- UI and shell;
- network policy;
- VPN control plane;
- IDS/IPS analysis engine;
- update orchestration;
- package management;
- recovery UI;
- logging presentation;
- policy engine;
- cryptographic policy and certificate management;
- identity workflows;
- configuration;
- telemetry;
- remote administration UI;
- Tor / I2P / mixnet clients;
- DNS / HTTP application logic;
- storage formats and filesystem services where architecture permits;
- hardware inventory presentation;
- evidence rendering;
- developer tooling.

## Security principle

Kernel code is part of the highest-value Trusted Computing Base.

Therefore:

```text
smaller kernel
= smaller attack surface
= fewer privileged parser bugs
= easier verification
= easier fuzzing
= easier formal reasoning
```

No network parser, complex document parser, package parser, neural model, speech model, web client, or update manifest parser should run in kernel mode unless there is no viable alternative.

## Accessibility principle

Accessibility is primarily user-space because it needs rapid iteration and rich semantics.

The kernel provides only the minimum emergency path necessary to keep the machine operable when user-space accessibility fails.

Example:

```text
normal state:
  full OMNI screen reader + TTS + braille in userspace

kernel failure:
  minimal kernel emergency output
  -> fixed vocabulary / tones / serial / braille primitive
  -> recovery reason
  -> safe reboot/recovery command
```

## Privilege boundary

```text
OMNI-Operator
    |
    | semantic request
    v
User-space policy / confirmation
    |
    | bounded capability request
    v
OMNI Kernel
    |
    | validates caller + target + capability
    v
hardware / protected resource
```

The kernel accepts structured operation IDs and bounded parameters.

It must not expose a generic "execute arbitrary privileged command" primitive.

## Rule of placement

A component belongs in the kernel only if at least one of the following is true:

1. it must execute before user-space exists;
2. it must enforce memory/process/device isolation;
3. it must access hardware that cannot safely be delegated;
4. it must remain trusted when user-space is compromised;
5. it must guarantee a property user-space must not be able to bypass.

Otherwise it belongs in user-space.

## Examples

| Function | Placement | Reason |
|---|---|---|
| Page tables | Kernel | non-bypassable memory isolation |
| IOMMU programming | Kernel | DMA enforcement |
| TPM command broker | Kernel/minimal trusted layer | hardware-root mediation |
| Raw disk write gate | Kernel | prevent untrusted direct writes |
| UEFI variable mutation | Kernel/trusted pre-OS layer | privileged platform state |
| Full TTS | User-space | complex, replaceable, large attack surface |
| Braille translation | User-space | rich semantics |
| IDS analysis | User-space | complex parser/analytics |
| Firewall enforcement hook | Kernel | non-bypassable packet enforcement |
| Firewall policy UI | User-space | policy and presentation |
| VPN crypto datapath | Kernel or isolated privileged component | performance/enforcement dependent |
| VPN negotiation/control | User-space | complex protocol logic |
| Update verification policy | User-space + kernel enforcement gate | split policy from protected mutation |
| Recovery UI | User-space/recovery environment | rich interaction |
| Panic output | Kernel | must survive user-space failure |

## Development rule

Any proposal to add code to OMNI Kernel must answer:

- Why can this not live in user-space?
- What security property becomes non-bypassable by putting it in the kernel?
- What new attack surface does it add?
- How is it fuzzed/tested?
- What happens if it crashes?
- What is the accessible failure path?
