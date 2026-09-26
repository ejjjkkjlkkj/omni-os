"""ST acoustic renderer for VoiceCore (host side).

VoiceCore owns text normalization (``voice_frontend``) and the PCM contract
(``voice_quality``). ST supplies the missing acoustic renderer through its
versioned C ABI (``st_synth`` v1): a persistent engine per voice, streaming
48 kHz float chunks, thread-safe cancellation.

The ST library is located with ``OMNI_ST_LIB`` (path to ``st_synth.dll`` /
``libst_synth.so``) or ``OMNI_ST_HOME`` (an ST release directory). Nothing is
downloaded and no ST binary is vendored in this repository.
"""
from __future__ import annotations

import ctypes
import json
import os
import sys
import threading
import time
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path

from .voice_frontend import Language, SpeechToken, TokenKind, normalize_for_speech
from .voice_quality import PCM_SAMPLE_RATE, PcmQualityReport, inspect_pcm16le

ST_ABI_RATE = 48_000
MAX_TEXT_BYTES = 65_536
_CODES = {1: "invalid input", 2: "backend failure", 3: "busy or internal panic", 4: "cancelled"}
_CLAUSE_TEXT = {
    "statement": ".",
    "exclamation": "!",
    "question": "?",
    "comma": ",",
    "semicolon": ";",
    "colon": ":",
}
_AUDIO_CALLBACK = ctypes.CFUNCTYPE(
    ctypes.c_uint8, ctypes.POINTER(ctypes.c_float), ctypes.c_size_t, ctypes.c_uint32, ctypes.c_void_p
)

assert ST_ABI_RATE == PCM_SAMPLE_RATE, "ST and VoiceCore must agree on the PCM rate"


class StError(RuntimeError):
    def __init__(self, code: int, message: str):
        super().__init__(f"ST {_CODES.get(code, 'error')} ({code}): {message}")
        self.code = code


class StCancelled(StError):
    pass


def tokens_to_text(tokens: Iterable[SpeechToken]) -> str:
    """Render VoiceCore frontend tokens as plain text for ST.

    Clause tokens become punctuation so ST keeps its phrasing and intonation;
    pause tokens become a comma (short prosodic break).
    """
    out: list[str] = []
    for token in tokens:
        if token.kind is TokenKind.CLAUSE:
            mark = _CLAUSE_TEXT[token.text]
            if out:
                out[-1] += mark
            continue
        if token.kind is TokenKind.PAUSE:
            if out and out[-1][-1:] not in ",.;:!?":
                out[-1] += ","
            continue
        out.append(token.text)
    return " ".join(out)


def config_json(*, lang: Language | str, backend: str = "neural", voice: str | None = None, rate: int = 100) -> bytes:
    lang = Language(lang)
    if backend not in ("neural", "compact"):
        raise ValueError("backend must be neural or compact")
    if not 50 <= rate <= 300:
        raise ValueError("rate must be 50..300")
    config: dict[str, object] = {"backend": backend, "lang": lang.value, "rate": rate}
    if voice:
        config["voice"] = voice
    return json.dumps(config).encode("utf-8")


def float_to_pcm16le(samples: Iterable[float]) -> bytes:
    """Round-to-nearest float [-1, 1) -> signed 16-bit LE, saturating at +/-32767.

    Saturation avoids the -32768/32767 codes that ``inspect_pcm16le`` counts as
    clipping; ST's master peaks at -1 dBFS so it is never reached in practice.
    """
    out = bytearray()
    for x in samples:
        v = int(round(x * 32767.0))
        v = 32766 if v > 32766 else -32766 if v < -32766 else v
        out += v.to_bytes(2, "little", signed=True)
    return bytes(out)


def pcm16le_wav(pcm: bytes, *, sample_rate: int = PCM_SAMPLE_RATE, channels: int = 1) -> bytes:
    if len(pcm) % (2 * channels):
        raise ValueError("PCM buffer is not frame-aligned")
    header = b"RIFF" + (36 + len(pcm)).to_bytes(4, "little") + b"WAVEfmt "
    header += (16).to_bytes(4, "little") + (1).to_bytes(2, "little") + channels.to_bytes(2, "little")
    header += sample_rate.to_bytes(4, "little") + (sample_rate * 2 * channels).to_bytes(4, "little")
    header += (2 * channels).to_bytes(2, "little") + (16).to_bytes(2, "little")
    return header + b"data" + len(pcm).to_bytes(4, "little") + pcm


def find_library() -> Path:
    explicit = os.environ.get("OMNI_ST_LIB")
    if explicit:
        return Path(explicit)
    home = os.environ.get("OMNI_ST_HOME")
    if not home:
        raise FileNotFoundError("Set OMNI_ST_LIB or OMNI_ST_HOME to an ST 0.6+ release")
    name = "st_synth.dll" if sys.platform == "win32" else "libst_synth.so"
    return Path(home) / name


class _Abi:
    def __init__(self, path: Path):
        self.lib = ctypes.CDLL(str(path))
        lib = self.lib
        missing = [name for name in ("st_engine_create_v1", "st_engine_stream_v1", "st_engine_cancel_v1",
                                     "st_engine_destroy_v1", "st_last_error_v1") if not hasattr(lib, name)]
        if missing:
            raise RuntimeError(f"{path} is too old for VoiceCore (needs ST >= 0.6.0-rc.2): missing {missing}")
        lib.st_engine_create_v1.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
        lib.st_engine_create_v1.restype = ctypes.c_int32
        lib.st_engine_stream_v1.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t, _AUDIO_CALLBACK, ctypes.c_void_p]
        lib.st_engine_stream_v1.restype = ctypes.c_int32
        lib.st_engine_cancel_v1.argtypes = [ctypes.c_void_p]
        lib.st_engine_cancel_v1.restype = None
        lib.st_engine_destroy_v1.argtypes = [ctypes.c_void_p]
        lib.st_engine_destroy_v1.restype = None
        lib.st_last_error_v1.argtypes = [ctypes.c_char_p, ctypes.c_size_t]
        lib.st_last_error_v1.restype = ctypes.c_size_t

    def last_error(self) -> str:
        buf = ctypes.create_string_buffer(1024)
        self.lib.st_last_error_v1(buf, len(buf))
        return buf.value.decode("utf-8", "replace")


@dataclass(frozen=True, slots=True)
class RenderResult:
    pcm16le: bytes
    report: PcmQualityReport
    first_audio_ms: float
    total_ms: float
    text: str

    @property
    def seconds(self) -> float:
        return len(self.pcm16le) / 2 / PCM_SAMPLE_RATE


class StRenderer:
    """One persistent ST engine (one language + voice). Create once, reuse.

    ``speak`` blocks the calling thread while chunks are delivered to ``sink``;
    ``cancel`` may be called from any other thread (key press) and makes the
    running ``speak`` raise :class:`StCancelled` without unloading the model.
    """

    def __init__(self, *, lang: Language | str = Language.FR, backend: str = "neural",
                 voice: str | None = None, rate: int = 100, library: Path | None = None):
        self.lang = Language(lang)
        self._abi = _Abi(library or find_library())
        self._handle = ctypes.c_void_p()
        self._lock = threading.Lock()
        cfg = config_json(lang=self.lang, backend=backend, voice=voice, rate=rate)
        code = self._abi.lib.st_engine_create_v1(cfg, len(cfg), ctypes.byref(self._handle))
        if code:
            raise StError(code, self._abi.last_error())

    def close(self) -> None:
        with self._lock:
            if self._handle:
                self._abi.lib.st_engine_destroy_v1(self._handle)
                self._handle = ctypes.c_void_p()

    def __enter__(self) -> "StRenderer":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def cancel(self) -> None:
        if self._handle:
            self._abi.lib.st_engine_cancel_v1(self._handle)

    def prepare_text(self, text: str, *, normalize: bool = True) -> str:
        return tokens_to_text(normalize_for_speech(text, self.lang)) if normalize else text

    def speak(self, text: str, sink: Callable[[memoryview], bool | None], *, normalize: bool = True) -> str:
        """Stream ``text``; ``sink`` gets float32 48 kHz chunks and may return False to stop."""
        spoken = self.prepare_text(text, normalize=normalize)
        data = spoken.encode("utf-8")
        if not data or len(data) > MAX_TEXT_BYTES:
            raise ValueError("text must be 1..65536 UTF-8 bytes")
        failure: list[BaseException] = []

        def on_audio(pcm, n, rate, _user):
            try:
                if rate != ST_ABI_RATE:
                    raise ValueError(f"unexpected ST rate {rate}")
                chunk = memoryview((ctypes.c_float * n).from_address(ctypes.addressof(pcm.contents))).cast("B").cast("f")
                return 0 if sink(chunk) is False else 1
            except BaseException as e:  # never unwind through the C ABI
                failure.append(e)
                return 0

        callback = _AUDIO_CALLBACK(on_audio)
        with self._lock:
            if not self._handle:
                raise RuntimeError("renderer is closed")
            code = self._abi.lib.st_engine_stream_v1(self._handle, data, len(data), callback, None)
            message = self._abi.last_error() if code else ""
        if failure:
            raise failure[0]
        if code == 4:
            raise StCancelled(code, message)
        if code:
            raise StError(code, message)
        return spoken

    def render(self, text: str, *, normalize: bool = True) -> RenderResult:
        """Whole utterance as PCM16LE mono 48 kHz, checked by the VoiceCore PCM gate."""
        floats: list[float] = []
        first: list[float] = []
        start = time.perf_counter()

        def sink(chunk: memoryview) -> bool:
            if not first:
                first.append((time.perf_counter() - start) * 1000)
            floats.extend(chunk)
            return True

        spoken = self.speak(text, sink, normalize=normalize)
        total = (time.perf_counter() - start) * 1000
        pcm = float_to_pcm16le(floats)
        return RenderResult(pcm, inspect_pcm16le(pcm), first[0] if first else total, total, spoken)
