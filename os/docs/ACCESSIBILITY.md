# Accessibility architecture

Accessibility is a platform invariant, not a desktop add-on.

## Required semantic contract

Every native interactive control must expose at least:

- stable node identifier;
- role;
- accessible name;
- value when meaningful;
- state;
- parent/child relationships;
- screen bounds;
- supported actions;
- keyboard focusability;
- change events.

A visual control that cannot expose the required semantic contract is considered invalid native UI.

The initial `aw-accessibility` crate already rejects unnamed interactive controls, interactive controls without a focus contract, focused nodes that are not focusable, and zero-sized interactive controls.

## System-wide availability

The long-term accessibility stack must operate in:

- installer;
- login/session creation;
- desktop;
- settings;
- recovery environment;
- update/rollback UI;
- crash and safe modes.

## Screen reader

The native screen reader should consume the same semantic event stream exposed to automation clients rather than scraping pixels or application internals.

Planned output layers:

- speech;
- braille;
- optional sound cues;
- structured automation API.

OCR remains a fallback for inaccessible foreign content, never the primary native accessibility mechanism.

## Keyboard contract

Every operation exposed by first-party UI must be reachable without a pointing device. Focus order must be deterministic and programmatically inspectable.

## Compatibility

Future compatibility adapters may expose semantic information through APIs expected by Windows applications and assistive technologies. Those adapters are separate from the native semantic tree so legacy compatibility cannot constrain the core accessibility model.
