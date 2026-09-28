from __future__ import annotations

from .voice_frontend import Language, TokenKind, normalize_for_speech
from .voice_phonemes import (
    FRONTEND_STREAM_VERSION,
    PhonemeId,
    frontend_phonemes,
    validate_frontend_stream,
)

MAX_TEXT_CHARS = 4096
MAX_STREAM_BYTES = 32768

_CLAUSE_MARK = {
    "statement": ".",
    "exclamation": "!",
    "question": "?",
    "comma": ",",
    "semicolon": ";",
    "colon": ":",
}


def compile_speech_stream(text: str, language: Language | str = Language.FR) -> bytes:
    """Compile accessibility text into the bounded VoiceCore phoneme/event stream."""
    if len(text) > MAX_TEXT_CHARS:
        raise ValueError("speech text exceeds firmware frontend limit")

    lang = Language(language)
    tokens = normalize_for_speech(text, lang)
    out = bytearray()
    have_spoken_segment = False

    for token in tokens:
        if token.kind is TokenKind.CLAUSE:
            mark = _CLAUSE_MARK[token.text]
            out.extend(frontend_phonemes(mark, lang.value))
            continue

        if token.kind is TokenKind.PAUSE:
            out.append(int(PhonemeId.PAUSE))
            continue

        if have_spoken_segment:
            out.extend((int(PhonemeId.PAUSE), int(PhonemeId.WORD_BOUNDARY)))

        if token.kind is TokenKind.ACRONYM:
            segment = token.text.replace(" ", "")
        else:
            segment = token.text

        out.extend(frontend_phonemes(segment, lang.value))
        have_spoken_segment = True

        if len(out) > MAX_STREAM_BYTES:
            raise ValueError("speech stream exceeds firmware frontend limit")

    stream = bytes(out)
    if not validate_frontend_stream(stream):
        raise ValueError("voice frontend produced an invalid stream")
    return stream
