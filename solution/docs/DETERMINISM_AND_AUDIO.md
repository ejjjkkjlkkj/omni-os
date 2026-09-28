# Determinism and audio gates

Navigation is software-complete only when the semantic graph is fully reachable, actions are deterministic, password values are redacted, and every non-terminal state can return to the root.

Cross-backend traces are normalized by role/name/value/state rather than volatile IDs or timestamps. QEMU, VMware and physical traces can therefore be compared without masking meaningful differences.

## Two different audio claims

The PCM gate is a **signal-integrity gate only**. It validates the WAV container and rejects empty, near-silent, clipped, DC-biased, structurally invalid, or undersampled mono PCM16 output. Its result is tagged `SIGNAL_INTEGRITY_ONLY` and always reports `speech_verified: false`.

A tone or beep is therefore allowed to pass signal integrity. It is never evidence that the screen reader spoke intelligibly.

The speech gate is separate. It requires:

- the signal-integrity gate to pass;
- a non-empty expected transcript;
- a non-empty transcript produced by an independent speech recognizer;
- an explicit recognizer identifier;
- word error rate at or below the configured threshold.

The default threshold is WER <= 0.15. Text comparison uses Unicode normalization, case folding, punctuation normalization, and deterministic word-level Levenshtein distance.

A waveform-only heuristic cannot set `speech_verified: true`.

## Evidence boundary

`analyze_speech_evidence` consumes recognition evidence; it is not itself an ASR engine. CI/HIL must run an independently identified recognizer on the audio capture and pass that recognizer's transcript to the gate. The signal result also carries the WAV SHA-256 so the capture can be content-addressed in the evidence bundle.

For virtual backends, this closes the software claim only when the captured synthesized output is independently recognized. For the ASUS physical path, microphone/loopback capture, HDA codec/amplifier/EAPD routing, speaker acoustics, and room/device effects remain hardware-in-the-loop evidence and must not be inferred from a generated PCM buffer.
