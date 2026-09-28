# VoiceCore v4 — Step 1 speech engine

VoiceCore v4 is a new implementation. It does not import or execute VoiceCore v2 or v3.

## Engineering target

The target is not an unsupported claim of being the world's best TTS. The target is a first-party engine that can be demonstrated to be unusually strong for pre-OS accessibility on measurable properties:

- no operating-system speech API;
- no cloud service;
- no external TTS runtime;
- deterministic output for reproducible firmware builds;
- 48 kHz signed 16-bit output suitable for native HDA;
- French text normalization including technical acronyms and integers;
- French grapheme-to-phoneme fallback plus explicit accessibility/firmware lexicon;
- sentence prosody and phrase-final pitch movement;
- neighbour-aware formant coarticulation;
- deterministic excitation/noise;
- multiple distinct voice profiles;
- clipping/DC regression gates;
- chunked streaming API for integration with bounded firmware buffers.

## Quality gates

A release of the engine must pass deterministic tests for every reference voice, maintain zero clipping in the reference corpus, keep DC below 1%, generate distinct output per voice, and preserve streaming/output equivalence.

Subjective intelligibility and naturalness must later be validated with listening tests. No CI acoustic metric alone is accepted as proof that VoiceCore is better than commercial or neural TTS systems.

## Step sequence

1. Speech engine core and voices.
2. Linguistic coverage expansion and pronunciation corpus.
3. UEFI streaming renderer and HDA binding.
4. Live HII screen-reader integration.
5. QEMU/OVMF and VMware listening validation.
6. Physical ASUS M1603QA pre-OS listening validation.
7. Blind-user navigation validation and release closure.
