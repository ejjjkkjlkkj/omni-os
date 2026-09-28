from __future__ import annotations

import hashlib
import math
import pathlib
import re
import struct
import unicodedata
import wave
from collections.abc import Sequence


def analyze_wav(path: str | pathlib.Path) -> dict[str, object]:
    """Validate transport/signal integrity only.

    This function intentionally does not claim that the waveform contains
    intelligible speech. A tone can pass this gate.
    """

    source = pathlib.Path(path)
    try:
        file_bytes = source.read_bytes()
        with wave.open(str(source), "rb") as wav:
            channels = wav.getnchannels()
            width = wav.getsampwidth()
            rate = wav.getframerate()
            frames = wav.getnframes()
            raw = wav.readframes(frames)
    except (OSError, EOFError, wave.Error) as exc:
        return {
            "status": "FAIL",
            "scope": "SIGNAL_INTEGRITY_ONLY",
            "speech_verified": False,
            "reason": "INVALID_WAV",
            "error": str(exc),
        }

    common: dict[str, object] = {
        "scope": "SIGNAL_INTEGRITY_ONLY",
        "speech_verified": False,
        "sha256": hashlib.sha256(file_bytes).hexdigest().upper(),
        "channels": channels,
        "sample_width": width,
        "sample_rate": rate,
    }

    if channels != 1 or width != 2:
        return {
            "status": "FAIL",
            "reason": "EXPECTED_MONO_PCM16",
            **common,
        }
    if rate <= 0:
        return {
            "status": "FAIL",
            "reason": "INVALID_SAMPLE_RATE",
            **common,
        }
    if not raw or len(raw) % 2:
        return {
            "status": "FAIL",
            "reason": "EMPTY_OR_TRUNCATED_AUDIO",
            **common,
        }

    samples = struct.unpack("<" + "h" * (len(raw) // 2), raw)
    count = len(samples)
    peak = max(abs(x) for x in samples)
    mean = sum(samples) / count
    rms = math.sqrt(sum(float(x) * x for x in samples) / count)
    silence = sum(abs(x) <= 128 for x in samples) / count
    clipping = sum(abs(x) >= 32760 for x in samples) / count
    max_step = max(
        (abs(samples[index] - samples[index - 1]) for index in range(1, count)),
        default=0,
    )
    duration = count / rate

    failures: list[str] = []
    if rate < 16000:
        failures.append("LOW_SAMPLE_RATE")
    if duration <= 0.02:
        failures.append("TOO_SHORT")
    if rms < 256:
        failures.append("NEAR_SILENCE")
    if abs(mean) > 2048:
        failures.append("EXCESSIVE_DC")
    if clipping > 0.001:
        failures.append("CLIPPING")
    if silence > 0.98:
        failures.append("MOSTLY_SILENT")

    return {
        "status": "PASS" if not failures else "FAIL",
        "failures": failures,
        **common,
        "samples": count,
        "duration_s": duration,
        "peak": peak,
        "rms": rms,
        "dc_mean": mean,
        "silence_fraction": silence,
        "clipping_fraction": clipping,
        "max_step": max_step,
    }


def normalize_transcript(text: str) -> list[str]:
    normalized = unicodedata.normalize("NFKC", text).casefold()
    normalized = normalized.replace("’", "'")
    normalized = re.sub(r"[^\w']+", " ", normalized, flags=re.UNICODE)
    return [token for token in normalized.split() if token]


def word_error_rate(
    reference: str | Sequence[str],
    hypothesis: str | Sequence[str],
) -> float:
    ref = normalize_transcript(reference) if isinstance(reference, str) else list(reference)
    hyp = normalize_transcript(hypothesis) if isinstance(hypothesis, str) else list(hypothesis)
    if not ref:
        raise ValueError("reference transcript must contain at least one word")

    previous = list(range(len(hyp) + 1))
    for ref_index, ref_word in enumerate(ref, start=1):
        current = [ref_index]
        for hyp_index, hyp_word in enumerate(hyp, start=1):
            substitution = previous[hyp_index - 1] + (ref_word != hyp_word)
            insertion = current[hyp_index - 1] + 1
            deletion = previous[hyp_index] + 1
            current.append(min(substitution, insertion, deletion))
        previous = current
    return previous[-1] / len(ref)


def analyze_speech_evidence(
    path: str | pathlib.Path,
    *,
    reference_text: str,
    recognized_text: str,
    recognizer: str,
    max_wer: float = 0.15,
) -> dict[str, object]:
    """Combine signal evidence with an independent transcript comparison.

    The recognizer is deliberately external to this module: a speech gate must
    consume independently produced recognition evidence rather than infer
    intelligibility from waveform energy or spectral activity alone.
    """

    signal = analyze_wav(path)
    failures: list[str] = []

    if signal.get("status") != "PASS":
        failures.append("SIGNAL_INTEGRITY_FAILED")
    if not isinstance(max_wer, (int, float)) or not 0.0 <= float(max_wer) <= 1.0:
        failures.append("INVALID_WER_THRESHOLD")

    ref_tokens = normalize_transcript(reference_text)
    hyp_tokens = normalize_transcript(recognized_text)
    recognizer_name = recognizer.strip()

    if not ref_tokens:
        failures.append("EMPTY_REFERENCE_TRANSCRIPT")
    if not hyp_tokens:
        failures.append("EMPTY_RECOGNIZED_TRANSCRIPT")
    if not recognizer_name:
        failures.append("MISSING_RECOGNIZER_ID")

    wer: float | None = None
    if ref_tokens and isinstance(max_wer, (int, float)) and 0.0 <= float(max_wer) <= 1.0:
        wer = word_error_rate(ref_tokens, hyp_tokens)
        if wer > float(max_wer):
            failures.append("WORD_ERROR_RATE_EXCEEDED")

    passed = not failures
    return {
        "status": "PASS" if passed else "FAIL",
        "scope": "SPEECH_EVIDENCE",
        "speech_verified": passed,
        "signal": signal,
        "reference_tokens": ref_tokens,
        "recognized_tokens": hyp_tokens,
        "recognizer": recognizer_name,
        "word_error_rate": wer,
        "max_word_error_rate": max_wer,
        "failures": failures,
    }
