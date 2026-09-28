# Accessible recovery contract

Status: mandatory design and validation contract for Accessible Windows recovery paths.

## Non-negotiable rule

A recovery environment is **not accessible** merely because it draws readable text on a framebuffer.
A blind user must be able to discover the failure, understand the available actions and operate the
recovery path without sight.

The implementation contract is represented by `aw-recovery-contract` and must remain independent
from any particular graphical shell.

## Minimum readiness

`AccessibleRecoveryReady` may be produced only when all of the following have passed runtime probes:

- deterministic keyboard input;
- structured machine-readable diagnostics;
- rollback selection to a known-good generation;
- reinstall from a verified/signed image;
- diagnostic export;
- at least one direct nonvisual output channel: speech or braille.

A visual console, framebuffer text, animation, icon or colour state never satisfies the nonvisual
output requirement.

## Success gating

Boot success and recovery accessibility are separate checks.

A generation may enter a trial boot when its immutable composition and anti-rollback policy pass.
It may become the new known-good `Successful` generation only after runtime checks pass for kernel,
storage, input, audio, accessibility broker, speech, security, updater and accessible recovery.

`RuntimeHealthReport` cannot mark the accessible-recovery check directly. It must receive the typed
`AccessibleRecoveryReady` proof. `aw-bootstate` then accepts `Trial -> Successful` only with a
`SuccessfulGeneration` proof for the exact selected generation and a rollback index at or above the
persisted floor.

Therefore a machine that reaches a graphical desktop while speech or accessible recovery is broken
must stay in trial state and must be eligible for automatic rollback.

## One diagnostic event, many frontends

Recovery failures use stable machine-readable codes and structured actions. The same event is fed to
visual text, speech, braille and technician/serial frontends. No frontend may invent a private error
state that is unavailable to the others.

Examples include boot-state corruption, rejected manifests, object-verification failure, storage
read failure, rollback activation, missing input/audio/speech/accessibility services and recovery
integrity failure.

Localized human wording is a presentation-layer concern. The stable diagnostic code and structured
action are the interoperability contract.

## Failure behaviour

- Never wait indefinitely for a graphical prompt that the user cannot perceive.
- Never require pointer input for a recovery-critical action.
- Never use colour, icon position or animation as the sole representation of state.
- Never mark an update successful before speech and recovery checks complete.
- Never discard the last accessible known-good generation while a new generation is still trial.
- If speech fails but braille is verified, braille may satisfy direct nonvisual output; if neither is
  verified, recovery readiness fails closed.
- Structured diagnostics must remain exportable even if the graphical shell does not start.
- Destructive actions require deterministic keyboard focus/order and an explicit confirmation model
  that can be spoken or represented in braille.
- A timeout must never silently select a destructive action or confirmation.
- Speech, braille, structured text and any visual frontend must expose the same diagnostic identity
  and action order so hidden visual state cannot surprise the user.

## Acceptance matrix

| Situation | Result |
| --- | --- |
| Desktop visible, speech unavailable | FAIL; generation remains trial |
| Desktop visible, accessible recovery unavailable | FAIL; generation remains trial |
| Recovery visible only on framebuffer | FAIL |
| Recovery has speech + keyboard + structured diagnostics + rollback/reinstall/export | PASS candidate |
| Recovery has braille instead of speech with the same controls/diagnostics | PASS candidate |
| Neither speech nor braille works | FAIL closed |
| Recovery proof belongs to another generation | FAIL |
| Health rollback index is below persisted floor | FAIL |
| Trial attempts exhausted | automatic fallback to previous known-good generation |

## Physical hardware evidence gate

The physical hardware helper must not turn a successful graphical/native-kernel boot into an
accessibility PASS. Its trusted DUT backend must emit all of the following exact evidence before the
lab can report `AW_ACCESSIBILITY_HARDWARE_GATE=PASS`:

- `AW_KEYBOARD_RECOVERY=PASS`;
- `AW_STRUCTURED_DIAGNOSTICS=PASS`;
- `AW_ACCESSIBLE_RECOVERY_FLOW=PASS`;
- `AW_NONVISUAL_ROLLBACK_FLOW=PASS`;
- exactly one direct-channel declaration matching `AW_NONVISUAL_CHANNEL=SPEECH`,
  `AW_NONVISUAL_CHANNEL=BRAILLE`, or `AW_NONVISUAL_CHANNEL=SPEECH+BRAILLE`.

These markers are evidence contracts, not placeholders that CI may synthesize. They must come from
the root-owned DUT cycle backend after it has exercised the corresponding behaviour on the physical
test machine. Missing, malformed or visual-only evidence fails closed.

This automated hardware gate still does **not** prove that a blind human can use the complete system.
It deliberately remains separate from the human walkthrough below so automation cannot overclaim
release-grade accessibility.

## Validation before release-grade claims

Automated tests are necessary but insufficient. Release-grade validation must include real blind-user
flows from power-on through failure and recovery, including at minimum:

1. booting a deliberately broken trial generation and hearing/reading the failure nonvisually;
2. selecting the previous known-good generation without sight;
3. corrupting or rejecting a manifest and receiving the same structured diagnostic through speech or
   braille and exported logs;
4. simulating speech failure and verifying braille fallback, then simulating both absent and verifying
   fail-closed readiness;
5. performing signed reinstall and diagnostic export using keyboard-only navigation;
6. power-loss/reboot during recovery/update without losing the last known-good accessible generation;
7. AMD and Intel physical x64 hardware tests, not only QEMU/OVMF;
8. a blind-user walkthrough in which every prompt, focus transition, confirmation, progress state,
   failure and rollback result is available without visual inspection.

Until these tests pass, the project may claim an implemented recovery contract, but not a fully
validated accessible recovery experience.
