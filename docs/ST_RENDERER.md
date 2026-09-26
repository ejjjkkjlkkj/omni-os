# ST acoustic renderer (host)

VoiceCore defines the speech contract (normalization, phoneme stream, 48 kHz PCM
gate) but ships no acoustic renderer. On the host (Windows), that renderer is
**ST ≥ 0.6.0-rc.2**, loaded through its versioned C ABI. No ST binary or model
is committed here; point the code at an ST release:

```powershell
$env:PYTHONPATH = "src"
$env:OMNI_ST_HOME = "C:\st\release\st-0.6.0-rc.2-windows-x64"   # or OMNI_ST_LIB = ...\st_synth.dll
python -m omni.cli voice-render "Menu Fichier, 3 éléments." --lang fr --out menu.wav
python -m unittest discover -s tests -p "test_voice*.py" -v
```

## Layers

```
semantic events ──► SemanticModel ──► announce.announcement()   (name, role, state, value; FR/EN)
                          │
                          ▼
                 announce.SpeechController ── focus change ⇒ interrupt + speak latest
                          │  (worker thread; caller never blocks)
                          ▼
     voice_frontend.normalize_for_speech ──► voice_st.tokens_to_text
                          │
                          ▼
              voice_st.StRenderer (ctypes → st_synth.dll, C ABI v1)
                 st_engine_create_v1 / stream_v1 / cancel_v1 / last_error_v1
                          │ float32 48 kHz chunks
                          ▼
     audio sink  ·  render(): PCM16LE ──► voice_quality.inspect_pcm16le (hard gate)
```

- Normalization is VoiceCore's, not ST's, so host and firmware speak the same words.
- `StRenderer` is one persistent engine per language/voice: create it at startup
  (the neural model loads once, ~3 s), then reuse it for every utterance.
- `cancel()` is thread-safe and keeps the model loaded. Measured on Ryzen 7 5800H:
  next utterance after an interruption ~0.4 s; repeated utterances ~0.1 ms (ST chunk cache).
- `SpeechController` never speaks a password value (`announce.announcement`).

## Backends

| backend | voices | first audio (new text) | footprint | licence notes |
|---|---|---|---|---|
| `neural` (default) | FR `ff_siwis`; EN `af_heart`, `af_bella`, `am_michael`, `bf_emma`, `bm_george` | 0.3–0.85 s | ~516 MB, private Python + ONNX Runtime, separate process | Kokoro weights Apache-2.0; phonemizer / eSpeak NG **GPL-3.0** (separate process) — see ST `LICENSE-THIRD-PARTY.md` |
| `compact` | `male`, `female`, `child` | 0.08–0.25 s | ~0.5 MB DLL, no runtime | ST code only; formant (robotic) timbre |

## UEFI boundary

The host renderer does **not** move into firmware as is:

- The neural backend needs a process, Python, ONNX Runtime and ~330 MB of fp32
  weights; none of this exists in a UEFI application.
- The compact backend is a Klatt formant synthesizer, and `ARCHITECTURE.md`
  keeps formant/Klatt parameters out of the target VoiceCore renderer.

What the firmware path reuses today is the contract, not the code: the same
normalization (`voice_frontend`), the same bounded phoneme stream
(`include/omni_voice_frontend.h`) and the same 48 kHz PCM gate. A firmware
renderer must consume that stream in a no-heap hot path and pass the same gate.
Making a neural renderer fit that budget (a small int8 acoustic model driven by
VoiceCore phoneme IDs instead of eSpeak) is future work; nothing in this
repository claims it.

## Evidence limits

`VOICE_PCM_CLEAN` proves transport hygiene only. Naturalness and intelligibility
still require human listening evidence, which has not been collected.
