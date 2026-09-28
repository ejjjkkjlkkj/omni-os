# VoiceCore v5 Neural

Objective: build a first-party neural TTS that can surpass VoiceCore v4 on naturalness while preserving accessibility latency and deterministic fallback.

## Product modes

- Studio: maximum naturalness and expressiveness, 48 kHz.
- Screen: streaming speech for screen readers, strict first-audio latency gate.
- UEFI fallback: VoiceCore v4 remains available when neural inference is not practical.

## Original female voices

- femme_naturelle: adult female voice, warm/neutral, technical-reading capable.
- jeune_femme_adulte: young adult female voice, clear/bright, expressive without exaggerated pitch.

No voice identity may be copied from a real person without explicit rights and consent.

## Neural architecture

1. French-first text frontend: Unicode normalization, numbers, units, paths, URLs, acronyms, grapheme+phoneme representation.
2. Speaker/style encoder: disentangles identity, prosody and recording conditions.
3. 48 kHz neural audio codec: RVQ discrete acoustic representation.
4. Slow semantic transformer: predicts semantic/acoustic-primary tokens over time.
5. Fast residual transformer: predicts residual codec codebooks per frame.
6. Prosody controller: duration, F0, energy, pauses, emphasis and emotion tags.
7. Streaming codec decoder: chunk-safe waveform synthesis.
8. Distilled Screen model: smaller low-latency student sharing the same voice identities.

## Scale plan

- v5-dev: 120M semantic + 40M residual for pipeline validation.
- v5-quality: 650M semantic + 160M residual.
- v5-flagship: 1.3B-2B semantic + 250M-400M residual if data and compute justify it.

## Non-negotiable gates

No SOTA claim until blind listening and technical benchmarks pass:
- French MOS / preference against selected contemporary references;
- intelligibility on UI, code, paths, URLs and technical acronyms;
- long-form stability;
- voice identity stability;
- no clipping / loudness regressions;
- streaming byte/frame continuity;
- TTFA target for Screen mode;
- documented dataset rights and speaker consent;
- separate evaluation set with no training leakage.

## Current state

VoiceCore v4 ABI 2 is the production fallback and baseline. VoiceCore v5 is the neural training track.
