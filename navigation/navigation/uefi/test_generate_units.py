#!/usr/bin/env python3
from __future__ import annotations

from generate_units import (
    DIGIT_UNITS, LETTER_UNITS, WORD_NAME_STRIDE, WORD_UNIT_STRIDE,
    WORD_UNITS, PHRASE_TEXTS, SOURCE_RATE, convert, load_source, make_source_units,
    _phrase_unit_name,
)

MAX_BANK_BYTES = 128 * 4096 - 0x1000


def main() -> None:
    speech = load_source()
    assert speech.SAMPLE_RATE == 48000
    assert 48000 % SOURCE_RATE == 0

    source_units = make_source_units(speech)
    required = (
        {"sil"}
        | {unit for seq in LETTER_UNITS.values() for unit in seq}
        | {unit for seq in DIGIT_UNITS.values() for unit in seq}
        | {unit for seq in WORD_UNITS.values() for unit in seq}
        | {_phrase_unit_name(i) for i in range(len(PHRASE_TEXTS))}
    )
    missing = sorted(required - set(source_units))
    assert not missing, f"missing native speech units: {missing}"

    assert set(LETTER_UNITS) == set("abcdefghijklmnopqrstuvwxyz")
    assert set(DIGIT_UNITS) == set("0123456789")
    assert max(map(len, LETTER_UNITS.values())) <= 8
    assert max(map(len, DIGIT_UNITS.values())) <= 8
    assert len(WORD_UNITS) >= 35
    assert {"boot","security","configuration","password","network","save","exit"} <= set(WORD_UNITS)
    assert len(WORD_UNITS) == len(set(WORD_UNITS))
    assert max(map(len, WORD_UNITS)) < WORD_NAME_STRIDE
    assert max(map(len, WORD_UNITS.values())) <= WORD_UNIT_STRIDE
    assert all(word.isascii() and word.islower() for word in WORD_UNITS)
    assert len(PHRASE_TEXTS) >= 6
    assert "ready press f1 for help" in PHRASE_TEXTS
    assert "no change" in PHRASE_TEXTS
    assert all(0 < len(phrase) <= 32 and phrase.isascii() for phrase in PHRASE_TEXTS)

    converted = {name: convert(source_units[name], SOURCE_RATE) for name in sorted(required)}
    assert all(data for data in converted.values())
    assert all(len(data) % 4 == 0 for data in converted.values()), "PCM must be stereo s16le"
    bank_bytes = sum(len(source_units[name]) for name in required)
    assert bank_bytes <= 1200 * 1024, bank_bytes

    silence = converted["sil"]
    assert silence == bytes(len(silence)), "converted silence must remain digital zero"

    print(f"source-rate={SOURCE_RATE}")
    print(f"unit-count={len(converted)}")
    print(f"bank-bytes={bank_bytes}")
    print(f"word-lexicon-count={len(WORD_UNITS)}")
    print(f"phrase-clip-count={len(PHRASE_TEXTS)}")
    print("VOICE_NAVIGATION_CONTRACT=PASS")


if __name__ == "__main__":
    main()
