#!/usr/bin/env python3
"""Verify exact PCM for completed UEFI navigation speech.

This proves emitted content, not human intelligibility. It pairs the F1, Down
and Up runtime speech strings from the serial log with the actual QEMU WAV,
reproduces the firmware's <=32-character word-boundary chunking, and requires
ordered, bit-identical captured speech.

Consecutive DMA chunks contain deliberate leading/trailing zero PCM. Those
silence regions can overlap as byte patterns in a continuous capture, so search
ordering advances to the end of each chunk's non-silent core. QEMU's WAV
backend may close the file before writing every final zero frame after the last
utterance. Only the final chunk may therefore use the strict tail-elision rule:
its complete leading silence plus non-silent core must be bit-identical, the
remaining capture must contain zeros only, and the omitted bytes must belong
exclusively to the expected trailing-zero region.
"""
from __future__ import annotations

import hashlib
import json
import sys
import wave
from pathlib import Path

import verify_speech_pcm as speech_pcm

KEY_MARKERS = {
    "HII_GRAPH_NAV_KEY=F1": "F1",
    "HII_GRAPH_NAV_KEY=DOWN": "DOWN",
    "HII_GRAPH_NAV_KEY=UP": "UP",
}
EXPECTED_KEYS = ("F1", "DOWN", "UP")
MAX_PHRASE = 64
MAX_CHUNK = 32
PCM_FRAME_BYTES = 4
ZERO_FRAME = bytes(PCM_FRAME_BYTES)


def split_runtime_phrase(text: str) -> list[str]:
    if not text or len(text) > MAX_PHRASE:
        raise ValueError("runtime navigation phrase must be 1..64 characters")

    chunks: list[str] = []
    offset = 0
    length = len(text)
    while offset < length:
        start = offset
        while start < length and text[start] == " ":
            start += 1
        if start >= length:
            break

        remaining = length - start
        take = min(remaining, MAX_CHUNK)
        if remaining > MAX_CHUNK:
            split = take
            while split > 0 and text[start + split] != " ":
                split -= 1
            if split >= 8:
                take = split

        while take and text[start + take - 1] == " ":
            take -= 1
        if not take:
            raise ValueError("runtime chunker produced an empty chunk")

        chunks.append(text[start : start + take])
        offset = start + take
        while offset < length and text[offset] == " ":
            offset += 1

    if not chunks:
        raise ValueError("runtime chunker produced no chunks")
    return chunks


def parse_navigation_speech(serial_path: Path) -> list[dict[str, str]]:
    pending: str | None = None
    found: dict[str, str] = {}
    for raw_line in serial_path.read_text(
        encoding="utf-8", errors="ignore"
    ).splitlines():
        line = raw_line.strip()
        key = KEY_MARKERS.get(line)
        if key is not None:
            pending = key
            continue
        prefix = "HII_GRAPH_NAV_SPEECH_TEXT="
        if pending and line.startswith(prefix):
            if pending not in found:
                found[pending] = line[len(prefix) :]
            pending = None

    missing = [key for key in EXPECTED_KEYS if key not in found]
    if missing:
        raise ValueError(f"missing runtime speech for keys: {','.join(missing)}")
    return [{"key": key, "text": found[key]} for key in EXPECTED_KEYS]


def wav_bytes(path: Path) -> tuple[dict[str, int | str], bytes]:
    with wave.open(str(path), "rb") as wav:
        params: dict[str, int | str] = {
            "channels": wav.getnchannels(),
            "sample_width_bytes": wav.getsampwidth(),
            "sample_rate_hz": wav.getframerate(),
            "compression": wav.getcomptype(),
            "frames": wav.getnframes(),
        }
        raw = wav.readframes(wav.getnframes())
    return params, raw


def outer_silence_bytes(pcm: bytes) -> tuple[int, int]:
    if len(pcm) % PCM_FRAME_BYTES:
        raise ValueError("expected PCM is not frame-aligned")
    start = 0
    end = len(pcm)
    while start < end and pcm[start : start + PCM_FRAME_BYTES] == ZERO_FRAME:
        start += PCM_FRAME_BYTES
    while end > start and pcm[end - PCM_FRAME_BYTES : end] == ZERO_FRAME:
        end -= PCM_FRAME_BYTES
    if start == end:
        raise ValueError("speech chunk contains no non-silent PCM")
    return start, len(pcm) - end


def find_frame_aligned(raw: bytes, expected: bytes, start: int) -> int:
    pos = raw.find(expected, start)
    while pos >= 0 and pos % PCM_FRAME_BYTES:
        pos = raw.find(expected, pos + 1)
    return pos


def final_zero_tail_match(
    raw: bytes,
    expected: bytes,
    cursor: int,
    leading_silence: int,
    trailing_silence: int,
) -> tuple[int, int, int] | None:
    """Accept only QEMU file-end elision of final trailing zero PCM.

    Returns (chunk_start, core_end, omitted_trailing_zero_bytes).
    """
    core_end_expected = len(expected) - trailing_silence
    prefix = expected[:core_end_expected]
    start = find_frame_aligned(raw, prefix, cursor)
    if start < 0:
        return None

    core_end = start + len(prefix)
    captured_tail = raw[core_end:]
    if len(captured_tail) > trailing_silence:
        return None
    if any(captured_tail):
        return None

    # The accepted prefix includes the exact generated leading silence and all
    # non-silent samples; only zero PCM at file end may be absent.
    if leading_silence <= 0 or core_end <= start + leading_silence:
        return None
    omitted = trailing_silence - len(captured_tail)
    return start, core_end, omitted


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print(
            "usage: verify_navigation_speech_pcm.py CAPTURE.wav SERIAL.log "
            "[REPORT.json]",
            file=sys.stderr,
        )
        return 2

    wav_path = Path(sys.argv[1])
    serial_path = Path(sys.argv[2])
    report_path = Path(sys.argv[3]) if len(sys.argv) == 4 else None

    events = parse_navigation_speech(serial_path)
    params, raw = wav_bytes(wav_path)
    format_ok = (
        params["channels"] == 2
        and params["sample_width_bytes"] == 2
        and params["sample_rate_hz"] == 48000
        and params["compression"] == "NONE"
    )

    discovery = speech_pcm.render_runtime_pcm(speech_pcm.DISCOVERY_TEXT)
    discovery_exact = raw.startswith(discovery)
    cursor = len(discovery) if discovery_exact else 0
    chunk_results: list[dict[str, object]] = []
    all_exact_content = format_ok and discovery_exact

    for event_index, event in enumerate(events):
        chunks = split_runtime_phrase(event["text"])
        for chunk_index, chunk in enumerate(chunks):
            expected = speech_pcm.render_runtime_pcm(chunk)
            leading_silence, trailing_silence = outer_silence_bytes(expected)
            core_bytes = len(expected) - leading_silence - trailing_silence
            is_final_chunk = (
                event_index == len(events) - 1
                and chunk_index == len(chunks) - 1
            )

            offset = (
                find_frame_aligned(raw, expected, cursor)
                if all_exact_content
                else -1
            )
            full_chunk_exact = offset >= 0
            tail_elision_accepted = False
            omitted_tail_bytes = 0

            if full_chunk_exact:
                core_end = offset + leading_silence + core_bytes
                content_exact = True
            elif all_exact_content and is_final_chunk:
                fallback = final_zero_tail_match(
                    raw,
                    expected,
                    cursor,
                    leading_silence,
                    trailing_silence,
                )
                if fallback is not None:
                    offset, core_end, omitted_tail_bytes = fallback
                    tail_elision_accepted = True
                    content_exact = True
                else:
                    core_end = -1
                    content_exact = False
            else:
                core_end = -1
                content_exact = False

            chunk_results.append(
                {
                    "key": event["key"],
                    "phrase": event["text"],
                    "chunk_index": chunk_index,
                    "chunk": chunk,
                    "expected_bytes": len(expected),
                    "expected_sha256": hashlib.sha256(expected).hexdigest(),
                    "leading_silence_bytes": leading_silence,
                    "non_silent_core_bytes": core_bytes,
                    "trailing_silence_bytes": trailing_silence,
                    "capture_offset_bytes": offset,
                    "ordered_core_end_bytes": core_end,
                    "full_chunk_bit_identical": full_chunk_exact,
                    "speech_content_bit_identical": content_exact,
                    "final_zero_tail_elision_accepted": tail_elision_accepted,
                    "omitted_trailing_zero_bytes": omitted_tail_bytes,
                }
            )
            if not content_exact:
                all_exact_content = False
                break

            # Do not skip a legitimate following chunk whose leading silence
            # begins inside this chunk's trailing all-zero region.
            cursor = core_end
        if not all_exact_content:
            break

    report = {
        "scope": "COMPLETED_F1_DOWN_UP_NAVIGATION_SPEECH",
        "ordering_rule": "ADVANCE_BY_END_OF_EXACT_SPEECH_CONTENT",
        "final_capture_rule": "ONLY_FINAL_TRAILING_ZERO_ELISION_ALLOWED",
        "wav": params,
        "discovery_prefix_exact": discovery_exact,
        "events": events,
        "chunks": chunk_results,
        "format_ok": format_ok,
        "full_navigation_speech_content": (
            "PASS" if all_exact_content else "FAIL"
        ),
        "intelligibility": "NOT_TESTED",
    }
    if report_path:
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    print(f"NAVIGATION_SPEECH_PCM_FORMAT={'PASS' if format_ok else 'FAIL'}")
    print(
        "NAVIGATION_DISCOVERY_PREFIX="
        + ("PASS" if discovery_exact else "FAIL")
    )
    print("NAVIGATION_SPEECH_KEYS=F1,DOWN,UP")
    print(
        "FULL_NAVIGATION_SPEECH_CONTENT="
        + ("PASS" if all_exact_content else "FAIL")
    )
    print("INTELLIGIBILITY=NOT_TESTED")
    return 0 if all_exact_content else 1


if __name__ == "__main__":
    raise SystemExit(main())
