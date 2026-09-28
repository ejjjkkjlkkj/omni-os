# VoiceCore frontend v1

The useful parts of the ST prototype are integrated as a renderer-independent frontend rather than as the final voice.

## What is retained

- deterministic French and English grapheme-to-phoneme rules;
- number expansion for values up to four digits and digit spelling beyond that;
- acronym and letter-name spelling;
- explicit word-boundary, pause and clause-end events;
- a compact stable one-byte phoneme/event stream;
- a matching C header for firmware consumers.

The formant/Klatt renderer is intentionally not imported into the core project. It remains useful only as an external bring-up or fallback voice. The target VoiceCore renderer should consume the frontend stream and apply its own duration, prosody, acoustic and vocoder stages.

## Contract

`FRONTEND_STREAM_VERSION = 1`

- IDs 1..51: speech phones.
- ID 52: word boundary.
- ID 53: clause end.
- ID 54: pause.
- ID 0: reserved/invalid.

Once a trained model or firmware image depends on this mapping, existing IDs must not be renumbered. Incompatible mapping changes require a stream-version increment.

## Python reference implementation

`src/omni/voice_frontend.py` exposes:

- `frontend_phonemes(text, language="fr") -> bytes`
- `validate_frontend_stream(stream) -> bool`
- `PhonemeId`
- `FRONTEND_STREAM_VERSION`

The implementation is deterministic and has no audio dependency. It can be used by host-side corpus preparation, test-vector generation, model input preparation and firmware fixture generation.

## Firmware boundary

`include/omni_voice_frontend.h` mirrors the exact numeric IDs. This lets the UEFI side consume precomputed/test phoneme streams without importing a new Rust toolchain into the software-ceiling build.

The intended pipeline is:

`UTF-8 text -> VoiceCore frontend -> phoneme/event stream -> duration/prosody -> acoustic representation -> primary vocoder -> PCM`

A compact fallback renderer can consume the same stream later, but it is not allowed to define the target voice quality.

## Validation

The dedicated tests lock the stream version, marker IDs, deterministic French output, English acronym spelling, number/clause handling, word boundaries and invalid-stream rejection.
