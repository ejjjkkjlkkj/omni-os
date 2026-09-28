#!/usr/bin/env python3
from __future__ import annotations
import hashlib
from native_speech_v4 import (
    SAMPLE_RATE, VOICES, PHONEMES, normalize_text, text_to_phonemes,
    synthesize, pcm_s16le_stereo, quality_metrics
)

def sha(samples):
    return hashlib.sha256(pcm_s16le_stereo(samples)).hexdigest()

def main():
    assert SAMPLE_RATE == 48000
    assert len(PHONEMES) >= 30
    assert {"clair","velours","screen","grave","femme","jeune_femme"} <= set(VOICES)
    assert VOICES["femme"].base_f0 > VOICES["clair"].base_f0
    assert VOICES["jeune_femme"].base_f0 > VOICES["femme"].base_f0
    assert VOICES["jeune_femme"].formant_scale > VOICES["femme"].formant_scale

    n = normalize_text("UEFI 2026, erreur !")
    assert "u e f i" in n and "deux zéro deux six" in n
    ph = text_to_phonemes("Bonjour, lecteur d'écran.")
    assert len(ph) > 10 and "ɔ̃" in ph and "ʁ" in ph

    phrase = "Bonjour. Lecteur d'écran prêt. UEFI, menu, continuer, récupération, erreur."
    hashes = {}
    for voice in VOICES:
        a = synthesize(phrase, voice)
        b = synthesize(phrase, voice)
        assert a == b, f"{voice}: nondeterministic"
        assert len(a) > SAMPLE_RATE
        m = quality_metrics(a)
        assert 0.008 < m["rms"] < 0.75, (voice, m)
        assert m["peak"] <= 0.95, (voice, m)
        assert m["dc"] < 0.01, (voice, m)
        assert m["clip_ratio"] == 0.0, (voice, m)
        hashes[voice] = sha(a)
        print(f"{voice}: duration={m['duration_s']:.3f}s rms={m['rms']:.6f} peak={m['peak']:.6f} sha256={hashes[voice]}")
    assert len(set(hashes.values())) == len(hashes)

    statement = synthesize("La navigation est prête.", "jeune_femme")
    question = synthesize("La navigation est prête ?", "jeune_femme")
    exclaim = synthesize("La navigation est prête !", "femme")
    assert sha(statement) != sha(question)
    assert sha(statement) != sha(exclaim)

    female = synthesize("Bonjour, je suis la voix féminine naturelle.", "femme")
    young = synthesize("Bonjour, je suis la voix féminine naturelle.", "jeune_femme")
    assert sha(female) != sha(young)
    print("VOICECORE_V4_TEST=PASS")

if __name__ == "__main__":
    main()
