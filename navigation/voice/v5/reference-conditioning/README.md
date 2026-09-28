# VoiceCore v5 — Reference voice conditioning

This directory defines the local-only ingestion path for a consented voice reference.

## Privacy rule

Raw voice recordings, cleaned recordings, speaker embeddings and user-specific profiles must not be committed to the public repository. Keep them outside Git or in ignored local paths.

## Pipeline

1. Verify that the recording may legally be used.
2. Convert to mono PCM WAV.
3. Trim silence and apply conservative noise reduction.
4. Resample the training copy to 48 kHz.
5. Extract F0, voiced ratio, spectral statistics and MFCC statistics.
6. Create a local profile JSON conforming to `reference-profile.schema.json`.
7. Segment the recording and attach exact human-verified transcripts.
8. Use the profile only as a conditioning seed; never claim exact cloning from a short sample.

## Data threshold

A single short sentence is sufficient for an acoustic seed and approximate parametric adaptation. It is not sufficient for a stable neural voice. VoiceCore treats the neural gate as unresolved until substantially more clean, varied and consented speech is available.

## Local command

```powershell
python extract_reference_profile.py --audio "reference.wav" --profile-id reference_femme_01 --out reference_femme_01.local.json
```

The output name intentionally ends in `.local.json`; local profiles and recordings must remain untracked.
