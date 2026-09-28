from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import re


class Language(str, Enum):
    FR = "fr"
    EN = "en"


class TokenKind(str, Enum):
    WORD = "word"
    NUMBER = "number"
    VERSION = "version"
    ACRONYM = "acronym"
    CLAUSE = "clause"
    PAUSE = "pause"


@dataclass(frozen=True, slots=True)
class SpeechToken:
    kind: TokenKind
    text: str


# A dotted numeric run (firmware version or decimal, e.g. "1.2.3" or "3.5") is
# captured as one token *before* word/punctuation splitting, so its internal dots
# are never mistaken for sentence-ending clauses.
_VERSION_RE = re.compile(r"[0-9]+(?:\.[0-9]+)+")
_TOKEN_RE = re.compile(
    r"[0-9]+(?:\.[0-9]+)+"
    r"|[A-Za-zÀ-ÖØ-öø-ÿ0-9]+(?:['’-][A-Za-zÀ-ÖØ-öø-ÿ0-9]+)*"
    r"|[.,!?;:]"
)
_CLAUSE = {
    ".": "statement",
    "!": "exclamation",
    "?": "question",
    ",": "comma",
    ";": "semicolon",
    ":": "colon",
}

_FR_ONES = ("zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf")
_FR_TEENS = ("dix", "onze", "douze", "treize", "quatorze", "quinze", "seize")
_FR_TENS = {20: "vingt", 30: "trente", 40: "quarante", 50: "cinquante", 60: "soixante"}
_EN_ONES = (
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
    "seventeen", "eighteen", "nineteen",
)
_EN_TENS = {
    20: "twenty", 30: "thirty", 40: "forty", 50: "fifty",
    60: "sixty", 70: "seventy", 80: "eighty", 90: "ninety",
}


def _is_acronym(value: str) -> bool:
    letters = [c for c in value if c.isascii() and c.isalpha()]
    return 1 < len(letters) <= 8 and len(letters) == len(value) and all(c.isupper() for c in letters)


def _spell_acronym(value: str, language: Language) -> str:
    del language
    return " ".join(value)


def _fr_under_100(n: int) -> str:
    if n < 10:
        return _FR_ONES[n]
    if n <= 16:
        return _FR_TEENS[n - 10]
    if n < 20:
        return f"dix-{_FR_ONES[n - 10]}"
    if n < 70:
        tens = (n // 10) * 10
        unit = n % 10
        base = _FR_TENS[tens]
        if unit == 0:
            return base
        if unit == 1:
            return f"{base} et un"
        return f"{base}-{_FR_ONES[unit]}"
    if n < 80:
        rest = n - 60
        if rest == 11:
            return "soixante et onze"
        return f"soixante-{_fr_under_100(rest)}"
    rest = n - 80
    base = "quatre-vingts" if rest == 0 else "quatre-vingt"
    return base if rest == 0 else f"{base}-{_fr_under_100(rest)}"


def _fr_number(n: int) -> str:
    if n < 100:
        return _fr_under_100(n)
    if n < 1000:
        hundreds, rest = divmod(n, 100)
        head = "cent" if hundreds == 1 else f"{_FR_ONES[hundreds]} cent"
        if rest == 0:
            if hundreds > 1:
                head += "s"
            return head
        return f"{head} {_fr_under_100(rest)}"
    if n < 1_000_000:
        thousands, rest = divmod(n, 1000)
        head = "mille" if thousands == 1 else f"{_fr_number(thousands)} mille"
        return head if rest == 0 else f"{head} {_fr_number(rest)}"
    return " ".join(_FR_ONES[int(d)] for d in str(n))


def _en_under_100(n: int) -> str:
    if n < 20:
        return _EN_ONES[n]
    tens = (n // 10) * 10
    unit = n % 10
    return _EN_TENS[tens] if unit == 0 else f"{_EN_TENS[tens]}-{_EN_ONES[unit]}"


def _en_number(n: int) -> str:
    if n < 100:
        return _en_under_100(n)
    if n < 1000:
        hundreds, rest = divmod(n, 100)
        head = f"{_EN_ONES[hundreds]} hundred"
        return head if rest == 0 else f"{head} {_en_under_100(rest)}"
    if n < 1_000_000:
        thousands, rest = divmod(n, 1000)
        head = f"{_en_number(thousands)} thousand"
        return head if rest == 0 else f"{head} {_en_number(rest)}"
    return " ".join(_EN_ONES[int(d)] for d in str(n))


def expand_number(value: str, language: Language | str) -> str:
    lang = Language(language)
    if not value.isascii() or not value.isdigit():
        raise ValueError("number must contain ASCII digits only")
    if len(value) > 18:
        digits = _FR_ONES if lang is Language.FR else _EN_ONES
        return " ".join(digits[int(d)] for d in value)
    n = int(value, 10)
    return _fr_number(n) if lang is Language.FR else _en_number(n)


def expand_version(value: str, language: Language | str) -> str:
    """Speak a dotted numeric run (version or decimal) as components joined by "point".

    Each component is expanded as a whole number, so "1.2.3" becomes
    "un point deux point trois" instead of three sentence-ended digits.
    """
    lang = Language(language)
    if not _VERSION_RE.fullmatch(value):
        raise ValueError("version must be digit groups separated by single dots")
    return " point ".join(expand_number(part, lang) for part in value.split("."))


def normalize_for_speech(
    text: str,
    language: Language | str = Language.FR,
) -> list[SpeechToken]:
    """Convert accessibility text into deterministic renderer-neutral speech tokens."""
    lang = Language(language)
    out: list[SpeechToken] = []
    for raw in _TOKEN_RE.findall(text):
        if raw in _CLAUSE:
            out.append(SpeechToken(TokenKind.CLAUSE, _CLAUSE[raw]))
            continue
        if "." in raw:
            out.append(SpeechToken(TokenKind.VERSION, expand_version(raw, lang)))
            continue
        if raw.isascii() and raw.isdigit():
            out.append(SpeechToken(TokenKind.NUMBER, expand_number(raw, lang)))
        elif _is_acronym(raw):
            out.append(SpeechToken(TokenKind.ACRONYM, _spell_acronym(raw, lang)))
        else:
            out.append(SpeechToken(TokenKind.WORD, raw))
    return out
