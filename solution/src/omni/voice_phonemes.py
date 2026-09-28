from __future__ import annotations

from enum import IntEnum

FRONTEND_STREAM_VERSION = 1


class PhonemeId(IntEnum):
    AA = 1
    AE = 2
    AH = 3
    AO = 4
    EH = 5
    ER = 6
    IH = 7
    IY = 8
    UH = 9
    UW = 10
    EY = 11
    AY = 12
    OY = 13
    AW = 14
    OW = 15
    M = 16
    N = 17
    NG = 18
    L = 19
    R = 20
    W = 21
    Y = 22
    F = 23
    V = 24
    S = 25
    Z = 26
    SH = 27
    ZH = 28
    TH = 29
    DH = 30
    HH = 31
    CH = 32
    JH = 33
    P = 34
    B = 35
    T = 36
    D = 37
    K = 38
    G = 39
    FR_A = 40
    FR_E_CLOSE = 41
    FR_O_CLOSE = 42
    FR_Y = 43
    EU = 44
    EU_OPEN = 45
    SCHWA = 46
    NAN = 47
    NON = 48
    NIN = 49
    NUN = 50
    NY = 51
    WORD_BOUNDARY = 52
    CLAUSE_END = 53
    PAUSE = 54


P = PhonemeId

_LETTER_EN = {
    "a": (P.EY,), "b": (P.B, P.IY), "c": (P.S, P.IY), "d": (P.D, P.IY),
    "e": (P.IY,), "f": (P.EH, P.F), "g": (P.JH, P.IY), "h": (P.EY, P.CH),
    "i": (P.AY,), "j": (P.JH, P.EY), "k": (P.K, P.EY), "l": (P.EH, P.L),
    "m": (P.EH, P.M), "n": (P.EH, P.N), "o": (P.OW,), "p": (P.P, P.IY),
    "q": (P.K, P.Y, P.UW), "r": (P.AA, P.R), "s": (P.EH, P.S),
    "t": (P.T, P.IY), "u": (P.Y, P.UW), "v": (P.V, P.IY),
    "w": (P.D, P.AH, P.B, P.AH, P.L, P.Y, P.UW), "x": (P.EH, P.K, P.S),
    "y": (P.W, P.AY), "z": (P.Z, P.IY),
}

_LETTER_FR = {
    "a": (P.AA,), "b": (P.B, P.EH), "c": (P.S, P.EH), "d": (P.D, P.EH),
    "e": (P.UH,), "f": (P.EH, P.F), "g": (P.JH, P.EH), "h": (P.AA, P.SH),
    "i": (P.IY,), "j": (P.JH, P.IY), "k": (P.K, P.AA), "l": (P.EH, P.L),
    "m": (P.EH, P.M), "n": (P.EH, P.N), "o": (P.OW,), "p": (P.P, P.EH),
    "q": (P.K, P.UW), "r": (P.EH, P.R), "s": (P.EH, P.S), "t": (P.T, P.EH),
    "u": (P.UW,), "v": (P.V, P.EH), "w": (P.D, P.UH, P.B, P.L, P.UW, P.V, P.EH),
    "x": (P.IY, P.K, P.S), "y": (P.IY, P.G, P.R, P.EH, P.K), "z": (P.Z, P.EH),
}

_DIGIT_EN = {
    0: (P.Z, P.IH, P.R, P.OW), 1: (P.W, P.AH, P.N), 2: (P.T, P.UW),
    3: (P.TH, P.R, P.IY), 4: (P.F, P.AO, P.R), 5: (P.F, P.AY, P.V),
    6: (P.S, P.IH, P.K, P.S), 7: (P.S, P.EH, P.V, P.AH, P.N),
    8: (P.EY, P.T), 9: (P.N, P.AY, P.N),
}

_DIGIT_FR = {
    0: (P.Z, P.EH, P.R, P.OW), 1: (P.UH, P.N), 2: (P.D, P.UW),
    3: (P.T, P.R, P.AA), 4: (P.K, P.AA, P.T, P.R), 5: (P.S, P.IH, P.N, P.K),
    6: (P.S, P.IY, P.S), 7: (P.S, P.EH, P.T), 8: (P.W, P.IH, P.T),
    9: (P.N, P.UH, P.F),
}

_CLAUSE_END = frozenset(".!?,;:")
_FR_VOWELS = frozenset("aeiouyàâéèêëîïôùûœ")


def _append(out: list[P], seq: tuple[P, ...] | list[P]) -> None:
    out.extend(seq)


def _spell_char(char: str, french: bool, out: list[P]) -> None:
    if char.isascii() and char.isdigit():
        _append(out, (_DIGIT_FR if french else _DIGIT_EN)[int(char)])
    else:
        _append(out, (_LETTER_FR if french else _LETTER_EN).get(char.lower(), ()))


def _is_acronym(token: str) -> bool:
    letters = [c for c in token if c.isascii() and c.isalpha()]
    return bool(letters) and len(letters) <= 5 and all(c.isupper() for c in letters)


def _number_phones(digits: str, french: bool, out: list[P]) -> None:
    try:
        value = int(digits)
    except ValueError:
        value = -1
    if len(digits) <= 4 and value >= 0:
        if french:
            _number_words_fr(value, out)
        else:
            _number_words_en(value, out)
        return
    table = _DIGIT_FR if french else _DIGIT_EN
    for char in digits:
        if char.isdigit():
            _append(out, table[int(char)])
            out.append(P.PAUSE)


def _number_words_en(value: int, out: list[P]) -> None:
    if value == 0:
        _append(out, _DIGIT_EN[0])
        return
    thousands = (value // 1000) % 10
    hundreds = (value // 100) % 10
    remainder = value % 100
    if thousands:
        _append(out, _DIGIT_EN[thousands])
        _append(out, (P.TH, P.AW, P.Z, P.AH, P.N, P.D))
        out.append(P.PAUSE)
    if hundreds:
        _append(out, _DIGIT_EN[hundreds])
        _append(out, (P.HH, P.AH, P.N, P.D, P.R, P.AH, P.D))
        out.append(P.PAUSE)
    _tens_unit_en(remainder, out)


def _tens_unit_en(value: int, out: list[P]) -> None:
    if value == 0:
        return
    if value < 10:
        _append(out, _DIGIT_EN[value])
        return
    teen = {
        10: (P.T, P.EH, P.N), 11: (P.IH, P.L, P.EH, P.V, P.AH, P.N),
        12: (P.T, P.W, P.EH, P.L, P.V), 13: (P.TH, P.ER, P.T, P.IY, P.N),
        14: (P.F, P.AO, P.R, P.T, P.IY, P.N), 15: (P.F, P.IH, P.F, P.T, P.IY, P.N),
        16: (P.S, P.IH, P.K, P.S, P.T, P.IY, P.N),
        17: (P.S, P.EH, P.V, P.AH, P.N, P.T, P.IY, P.N),
        18: (P.EY, P.T, P.IY, P.N), 19: (P.N, P.AY, P.N, P.T, P.IY, P.N),
    }
    if value in teen:
        _append(out, teen[value])
        return
    tens = {
        2: (P.T, P.W, P.EH, P.N, P.T, P.IY), 3: (P.TH, P.ER, P.T, P.IY),
        4: (P.F, P.AO, P.R, P.T, P.IY), 5: (P.F, P.IH, P.F, P.T, P.IY),
        6: (P.S, P.IH, P.K, P.S, P.T, P.IY),
        7: (P.S, P.EH, P.V, P.AH, P.N, P.T, P.IY),
        8: (P.EY, P.T, P.IY), 9: (P.N, P.AY, P.N, P.T, P.IY),
    }
    _append(out, tens[value // 10])
    unit = value % 10
    if unit:
        out.append(P.PAUSE)
        _append(out, _DIGIT_EN[unit])


def _stressed_position_ahead(chars: list[str], position: int) -> bool:
    return any(c in "aeiouy" for c in chars[position + 1 :])


def _word_phones_en(word: str, out: list[P]) -> None:
    chars = [c.lower() for c in word if c.isascii() and c.isalpha()]
    if not chars:
        for c in word:
            if c.isascii() and c.isdigit():
                _spell_char(c, False, out)
        return
    n = len(chars)
    if not any(c in "aeiouy" for c in chars):
        for c in chars:
            _spell_char(c, False, out)
            out.append(P.PAUSE)
        return

    groups: list[tuple[int, int]] = []
    start: int | None = None
    for index, char in enumerate(chars):
        vowel = char in "aeiouy"
        if vowel and start is None:
            start = index
        elif not vowel and start is not None:
            groups.append((start, index))
            start = None
    if start is not None:
        groups.append((start, n))

    stressed = [False] * n
    if groups:
        for index in range(*groups[0]):
            stressed[index] = True
        if len(groups) >= 3:
            for index in range(*groups[-1]):
                stressed[index] = True

    def at(index: int) -> str:
        return chars[index] if 0 <= index < n else " "

    def is_vowel(char: str) -> bool:
        return char in "aeiou"

    i = 0
    while i < n:
        c, nxt, nxt2 = chars[i], at(i + 1), at(i + 2)
        unstressed_vowel = c in "aeiouy" and not stressed[i]
        consume = 0

        if c == "a":
            if i + 2 < n and not is_vowel(nxt) and nxt2 == "e" and i + 3 == n:
                phone = P.EY
            elif nxt in "iy":
                consume, phone = 1, P.EY
            elif nxt == "w" or (nxt == "u" and not is_vowel(nxt2)):
                consume, phone = 1, P.AO
            elif nxt == "r":
                phone = P.AA
            else:
                phone = P.AE
            out.append(P.SCHWA if unstressed_vowel else phone)
        elif c == "e":
            if i == n - 1 and n > 2:
                pass
            elif nxt in "ea":
                consume = 1
                out.append(P.SCHWA if unstressed_vowel else P.IY)
            elif nxt in "iy":
                consume = 1
                out.append(P.SCHWA if unstressed_vowel else P.EY)
            elif nxt == "r":
                out.append(P.ER)
            elif nxt == "w":
                consume = 1
                out.append(P.SCHWA if unstressed_vowel else P.UW)
            else:
                out.append(P.SCHWA if unstressed_vowel else P.EH)
        elif c == "i":
            if i + 2 < n and not is_vowel(nxt) and nxt2 == "e" and i + 3 == n:
                phone = P.AY
            elif nxt == "g" and nxt2 == "h":
                consume, phone = 2, P.AY
            elif nxt == "r":
                phone = P.ER
            else:
                phone = P.IH
            out.append(P.SCHWA if unstressed_vowel else phone)
        elif c == "o":
            if i + 2 < n and not is_vowel(nxt) and nxt2 == "e" and i + 3 == n:
                phone = P.OW
            elif nxt == "o":
                consume, phone = 1, P.UW
            elif nxt in "wu":
                consume, phone = 1, P.AW
            elif nxt in "iy":
                consume, phone = 1, P.OY
            elif nxt == "r":
                phone = P.AO
            else:
                phone = P.AA
            out.append(P.SCHWA if unstressed_vowel else phone)
        elif c == "u":
            if i + 2 < n and not is_vowel(nxt) and nxt2 == "e" and i + 3 == n:
                out.extend((P.Y, P.SCHWA if unstressed_vowel else P.UW))
            elif nxt == "r":
                out.append(P.ER)
            else:
                out.append(P.SCHWA if unstressed_vowel else P.AH)
        elif c == "y":
            if i == 0:
                out.append(P.Y)
            elif i == n - 1:
                out.append(P.IY)
            else:
                out.append(P.SCHWA if unstressed_vowel else P.IH)
        elif c == "s" and nxt == "h":
            out.append(P.SH); consume = 1
        elif c == "c" and nxt == "h":
            out.append(P.CH); consume = 1
        elif c == "t" and nxt == "h":
            out.append(P.DH if i == 0 else P.TH); consume = 1
        elif c == "p" and nxt == "h":
            out.append(P.F); consume = 1
        elif c == "g" and nxt == "h":
            consume = 1
        elif c == "c" and nxt == "k":
            out.append(P.K); consume = 1
        elif c == "n" and nxt == "g" and i + 2 == n:
            out.append(P.NG); consume = 1
        elif c == "q":
            out.append(P.K)
            if nxt == "u":
                out.append(P.W); consume = 1
        elif c == "c":
            out.append(P.S if nxt in "eiy" else P.K)
        elif c == "g":
            out.append(P.JH if nxt in "eiy" else P.G)
        elif c == "x":
            out.extend((P.K, P.S))
        elif c == "b": out.append(P.B)
        elif c == "d": out.append(P.D)
        elif c == "f": out.append(P.F)
        elif c == "h": out.append(P.HH)
        elif c == "j": out.append(P.JH)
        elif c == "k":
            out.append(P.K)
            if _stressed_position_ahead(chars, i): out.append(P.HH)
        elif c == "l": out.append(P.L)
        elif c == "m": out.append(P.M)
        elif c == "n": out.append(P.N)
        elif c == "p":
            out.append(P.P)
            if _stressed_position_ahead(chars, i): out.append(P.HH)
        elif c == "r": out.append(P.R)
        elif c == "s":
            previous_vowel = i > 0 and at(i - 1) in "aeiouy"
            rest = "".join(chars[i + 1 :])
            if previous_vowel and (rest.startswith("ion") or rest.startswith("ure")):
                out.append(P.ZH)
            elif previous_vowel and is_vowel(nxt):
                out.append(P.Z)
            else:
                out.append(P.S)
        elif c == "t":
            out.append(P.T)
            if _stressed_position_ahead(chars, i): out.append(P.HH)
        elif c == "v": out.append(P.V)
        elif c == "w": out.append(P.W)
        elif c == "z": out.append(P.Z)

        i += consume
        if i + 1 < n and chars[i + 1] == c and not is_vowel(c):
            i += 1
        i += 1


def _is_vowel_fr(char: str) -> bool:
    return char in _FR_VOWELS


def _word_phones_fr(word: str, out: list[P]) -> None:
    chars = list(word.lower())
    n = len(chars)
    if not n:
        return

    def at(index: int) -> str:
        return chars[index] if 0 <= index < n else "\0"

    def matches(index: int, value: str) -> bool:
        return "".join(chars[index : index + len(value)]) == value

    def ends(index: int, value: str) -> bool:
        return matches(index, value) and index + len(value) == n

    def nasal(index: int, consumed: int) -> bool:
        target = index + consumed
        if target >= n:
            return True
        c = chars[target]
        return c not in "nm" and not _is_vowel_fr(c)

    whole = "".join(chars)
    specials = {
        "et": (P.FR_E_CLOSE,), "six": (P.S, P.IY, P.S), "dix": (P.D, P.IY, P.S),
        "sept": (P.S, P.EH, P.T), "huit": (P.FR_Y, P.IY, P.T),
        "neuf": (P.N, P.EU_OPEN, P.F), "mille": (P.M, P.IY, P.L),
        "ville": (P.V, P.IY, P.L), "soixante": (P.S, P.W, P.FR_A, P.S, P.NAN, P.T),
        "windows": (P.W, P.NIN, P.D, P.FR_O_CLOSE, P.Z),
        "firmware": (P.F, P.IY, P.R, P.M, P.W, P.EH, P.R),
    }
    if whole in specials:
        _append(out, specials[whole])
        return

    i = 0
    while i < n:
        c, d, e = at(i), at(i + 1), at(i + 2)
        if matches(i, "eaux"):
            out.append(P.FR_O_CLOSE); i += 4
        elif matches(i, "eau"):
            out.append(P.FR_O_CLOSE); i += 3
        elif matches(i, "tion"):
            out.extend((P.S, P.Y, P.NON)); i += 4
        elif matches(i, "sion"):
            out.extend((P.Z, P.Y, P.NON)); i += 4
        elif matches(i, "oin") and nasal(i, 3):
            out.extend((P.W, P.NIN)); i += 3
        elif matches(i, "ien") and nasal(i, 3):
            out.extend((P.Y, P.NIN)); i += 3
        elif (matches(i, "ain") or matches(i, "ein")) and nasal(i, 3):
            out.append(P.NIN); i += 3
        elif matches(i, "sch"):
            out.append(P.SH); i += 3
        elif c == "c" and d == "h":
            out.append(P.SH); i += 2
        elif c == "p" and d == "h":
            out.append(P.F); i += 2
        elif c == "t" and d == "h":
            out.append(P.T); i += 2
        elif c == "g" and d == "n":
            out.append(P.NY); i += 2
        elif c == "n" and d == "g":
            out.append(P.NG); i += 2
        elif c == "q" and d == "u":
            out.append(P.K); i += 2
        elif c == "g" and d == "u" and e in "eiy":
            out.append(P.G); i += 2
        elif c == "o" and d == "i":
            out.extend((P.W, P.FR_A)); i += 2
        elif c == "o" and d == "u":
            out.append(P.UW); i += 2
        elif c == "a" and d == "u":
            out.append(P.FR_O_CLOSE); i += 2
        elif c in "aeoiyu" and d in "nm" and nasal(i, 2):
            out.append({"a": P.NAN, "e": P.NAN, "o": P.NON, "u": P.NUN}.get(c, P.NIN)); i += 2
        elif c in "ae" and d == "i":
            out.append(P.EH); i += 2
        elif c == "e" and d == "u":
            out.append(P.EU); i += 2
        elif c == "œ" and d == "u":
            out.append(P.EU_OPEN); i += 2
        elif c == "i" and d == "l" and e == "l":
            out.append(P.Y); i += 3
        elif ends(i, "er") or ends(i, "ez"):
            out.append(P.FR_E_CLOSE); i += 2
        elif i + 1 == n and c in "esxzdtpg":
            i += 1
        elif c == "h":
            i += 1
        else:
            if c in "aàâ": out.append(P.FR_A)
            elif c == "é": out.append(P.FR_E_CLOSE)
            elif c in "eèêë": out.append(P.SCHWA if i + 1 == n else P.EH)
            elif c in "iîï": out.append(P.IY)
            elif c in "oô": out.append(P.AO)
            elif c in "uùû": out.append(P.FR_Y)
            elif c == "y": out.append(P.Y if i > 0 and _is_vowel_fr(at(i - 1)) and _is_vowel_fr(d) else P.IY)
            elif c == "b": out.append(P.B)
            elif c == "d": out.append(P.D)
            elif c == "f": out.append(P.F)
            elif c == "g": out.append(P.ZH if d in "eiy" else P.G)
            elif c == "j": out.append(P.ZH)
            elif c in "kq": out.append(P.K)
            elif c in "cç": out.append(P.S if c == "ç" or d in "eiy" else P.K)
            elif c == "l": out.append(P.L)
            elif c == "m": out.append(P.M)
            elif c == "n": out.append(P.N)
            elif c == "p": out.append(P.P)
            elif c == "r": out.append(P.R)
            elif c == "s": out.append(P.Z if i > 0 and _is_vowel_fr(at(i - 1)) and _is_vowel_fr(d) else P.S)
            elif c == "t": out.append(P.T)
            elif c == "v": out.append(P.V)
            elif c == "w": out.append(P.W)
            elif c == "z": out.append(P.Z)
            elif c == "x": out.extend((P.K, P.S))
            i += 1


def _number_words_fr(value: int, out: list[P]) -> None:
    if value == 0:
        _word_phones_fr("zéro", out)
        return
    remaining = value
    if remaining >= 1000:
        thousands = remaining // 1000
        if thousands > 1:
            _number_under_1000_fr(thousands, out)
            out.append(P.PAUSE)
        _word_phones_fr("mille", out)
        out.append(P.PAUSE)
        remaining %= 1000
    if remaining:
        _number_under_1000_fr(remaining, out)


def _number_under_1000_fr(value: int, out: list[P]) -> None:
    remaining = value
    if remaining >= 100:
        hundreds = remaining // 100
        if hundreds > 1:
            _number_under_100_fr(hundreds, out)
            out.append(P.PAUSE)
        _word_phones_fr("cent", out)
        out.append(P.PAUSE)
        remaining %= 100
    if remaining:
        _number_under_100_fr(remaining, out)


def _number_under_100_fr(value: int, out: list[P]) -> None:
    ones = ("zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf")
    teens = ("dix", "onze", "douze", "treize", "quatorze", "quinze", "seize")
    tens = ("", "", "vingt", "trente", "quarante", "cinquante", "soixante")

    def word(value: str) -> None:
        _word_phones_fr(value, out)
        out.append(P.PAUSE)

    if value < 10:
        _word_phones_fr(ones[value], out)
    elif value <= 16:
        _word_phones_fr(teens[value - 10], out)
    elif value < 20:
        word("dix"); _word_phones_fr(ones[value - 10], out)
    elif value < 70:
        unit = value % 10
        word(tens[value // 10])
        if unit == 1: word("et")
        if unit: _word_phones_fr(ones[unit], out)
    elif value < 80:
        word("soixante")
        if value == 71: word("et")
        _number_under_100_fr(value - 60, out)
    else:
        word("quatre"); word("vingt")
        if value > 80: _number_under_100_fr(value - 80, out)


def frontend_phonemes(text: str, language: str = "fr") -> bytes:
    """Return the deterministic VoiceCore frontend stream.

    The stream is intentionally independent from any acoustic renderer. It is a
    compact contract that can feed the future VoiceCore duration/prosody/acoustic
    model or a firmware fallback voice.
    """
    language_key = language.strip().lower()
    if language_key not in {"fr", "en"}:
        raise ValueError("language must be 'fr' or 'en'")
    french = language_key == "fr"
    out: list[P] = []

    for token_index, token in enumerate(text.split()):
        if token_index:
            out.extend((P.PAUSE, P.WORD_BOUNDARY))
        trimmed = token.rstrip(".!?,;:")
        trailing = token[len(trimmed) :]

        if token.isascii() and token.isdigit():
            _number_phones(token, french, out)
        elif _is_acronym(trimmed):
            for char in trimmed:
                if char.isascii() and char.isalnum():
                    _spell_char(char, french, out)
                    out.append(P.PAUSE)
        elif french:
            _word_phones_fr(trimmed, out)
        else:
            _word_phones_en(trimmed, out)

        if trailing and all(char in _CLAUSE_END for char in trailing):
            out.extend((P.PAUSE, P.CLAUSE_END))

    return bytes(int(phone) for phone in out)


def validate_frontend_stream(stream: bytes | bytearray | memoryview) -> bool:
    data = bytes(stream)
    return all(1 <= value <= int(P.PAUSE) for value in data)
