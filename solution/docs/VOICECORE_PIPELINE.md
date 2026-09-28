# VoiceCore clean pipeline

The speech path is split into independent, testable layers.

1. `voice_frontend.py` normalizes HII/IFR accessibility text into semantic speech tokens.
2. `voice_phonemes.py` provides the stable FR/EN phoneme/event encoding and the firmware-facing stream contract.
3. `voice_pipeline.py` compiles semantic tokens into a bounded stream suitable for a firmware renderer.
4. `voice_quality.py` validates 48 kHz PCM transport hygiene before audio is accepted.

The firmware contract is mirrored by `include/omni_voice_frontend.h`. Existing phoneme/event IDs are versioned and must not be silently renumbered. The header also pins the text/stream bounds and the 48 kHz PCM channel ceiling; CI verifies those values stay identical to the Python reference implementation.

## Clean PCM gate

The hard PCM gate rejects:

- clipped signed-16-bit samples;
- excessive DC offset;
- silence or near-silence;
- malformed frame alignment;
- formats other than 48 kHz mono/stereo PCM16LE.

This gate deliberately does **not** claim that a voice is natural or intelligible. Those properties require the VoiceCore acoustic renderer plus objective acoustic checks and human listening evidence. A renderer is not considered release-ready merely because it produces syntactically valid PCM.

## Firmware bounds

The reference compiler caps text at 4096 characters and the generated frontend stream at 32768 bytes. Firmware implementations must enforce equivalent or stricter bounds without heap-dependent behavior in the hot path.
