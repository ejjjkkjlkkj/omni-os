#!/usr/bin/env python3
from __future__ import annotations

import hashlib
from voicecore_v4 import (
    DEFAULT_CHUNK_FRAMES,
    ENGINE_ABI,
    ENGINE_NAME,
    PHONES,
    SAMPLE_RATE,
    VOICES,
    engine_fingerprint,
    integer_to_words,
    normalize_text,
    pcm_s16le_stereo,
    quality_metrics,
    synthesize,
    synthesize_stream,
    text_to_events,
)

def digest(samples: list[int]) -> str:
    return hashlib.sha256(pcm_s16le_stereo(samples)).hexdigest()

def main() -> None:
    assert ENGINE_NAME == "VoiceCore v4"
    assert ENGINE_ABI == 2
    assert SAMPLE_RATE == 48000
    assert DEFAULT_CHUNK_FRAMES == 960
    assert len(PHONES) >= 38
    assert {"screen","clair","velours","grave","rapide","compact","femme","jeune_femme"} <= set(VOICES)
    assert VOICES["femme"].base_f0 > VOICES["clair"].base_f0
    assert VOICES["jeune_femme"].base_f0 > VOICES["femme"].base_f0

    assert integer_to_words(0) == "zéro"
    assert integer_to_words(21) == "vingt et un"
    assert integer_to_words(71) == "soixante onze"
    assert integer_to_words(2026) == "deux mille vingt six"

    normalized = normalize_text("UEFI 2026, HDA 48.5!")
    assert "u e f i" in normalized
    assert "deux mille vingt six" in normalized
    assert "h d a" in normalized
    assert "quarante huit virgule cinq" in normalized

    events = text_to_events("Bonjour, menu sécurité. UEFI USB HDA.")
    assert len(events) > 20
    assert any(e.symbol == "ɔ̃" for e in events)
    assert any(e.symbol == "ʁ" for e in events)
    assert all(e.symbol in PHONES for e in events)

    phrase = (
        "Bonjour. UEFI, menu sécurité. Lecteur d'écran audio. "
        "Continuer, récupération, erreur, non. USB PCI HDA TPM 2026."
    )
    hashes: dict[str, str] = {}
    for voice in VOICES:
        a = synthesize(phrase, voice)
        assert len(a) > SAMPLE_RATE
        m = quality_metrics(a)
        assert 0.005 < m["rms"] < 0.75, (voice, m)
        assert m["peak"] <= 0.95, (voice, m)
        assert m["dc"] < 0.01, (voice, m)
        assert m["clip_ratio"] == 0.0, (voice, m)
        assert 0.001 < m["zcr"] < 0.50, (voice, m)
        chunks = list(synthesize_stream(phrase, voice, 777))
        flat = [s for chunk in chunks for s in chunk]
        assert flat == a, f"{voice}: streaming must be deterministic and byte-identical"
        assert all(1 <= len(chunk) <= 777 for chunk in chunks)
        hashes[voice] = digest(a)
        print(
            f"{voice}: duration={m['duration_s']:.3f}s rms={m['rms']:.6f} "
            f"peak={m['peak']:.6f} zcr={m['zcr']:.6f} sha256={hashes[voice]}"
        )

    assert len(set(hashes.values())) == len(hashes)

    statement = synthesize("La navigation est prête.", "jeune_femme")
    question = synthesize("La navigation est prête ?", "jeune_femme")
    exclaim = synthesize("La navigation est prête !", "femme")
    assert digest(statement) != digest(question)
    assert digest(statement) != digest(exclaim)
    assert digest(synthesize("Bonjour, voix féminine.", "femme")) != digest(
        synthesize("Bonjour, voix féminine.", "jeune_femme")
    )

    fp = engine_fingerprint()
    assert len(fp) == 64
    print(f"engine-fingerprint={fp}")
    print("VOICECORE_V4_TEST=PASS")

if __name__ == "__main__":
    main()
