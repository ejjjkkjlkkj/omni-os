from __future__ import annotations

import hashlib
import json
import math
from collections.abc import Iterable, Mapping
from typing import Any

GENESIS = "0" * 64
SCHEME = "omni.flight.v1"
_HASH_DOMAIN = b"OMNI_FLIGHT_EVENT_V1\0"


class FlightError(ValueError):
    pass


def _validate_json(value: Any, path: str = "$") -> None:
    if value is None or isinstance(value, (bool, int, str)):
        return
    if isinstance(value, float):
        if not math.isfinite(value):
            raise FlightError(f"non-finite number at {path}")
        return
    if isinstance(value, list):
        for index, item in enumerate(value):
            _validate_json(item, f"{path}[{index}]")
        return
    if isinstance(value, dict):
        for key, item in value.items():
            if not isinstance(key, str):
                raise FlightError(f"non-string object key at {path}")
            _validate_json(item, f"{path}.{key}")
        return
    raise FlightError(f"unsupported JSON type at {path}: {type(value).__name__}")


def canonical(event: Mapping[str, object]) -> bytes:
    if not isinstance(event, Mapping):
        raise FlightError("event must be an object")
    plain = dict(event)
    _validate_json(plain)
    try:
        return json.dumps(
            plain,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
            allow_nan=False,
        ).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise FlightError(f"event is not canonical JSON: {exc}") from exc


def _decode_hash(value: object, field: str) -> bytes:
    if not isinstance(value, str) or len(value) != 64:
        raise FlightError(f"{field} must be a 64-character SHA-256 hex string")
    try:
        decoded = bytes.fromhex(value)
    except ValueError as exc:
        raise FlightError(f"{field} is not hexadecimal") from exc
    if len(decoded) != 32:
        raise FlightError(f"{field} must decode to 32 bytes")
    return decoded


def event_hash(previous: str, event: Mapping[str, object]) -> str:
    previous_bytes = _decode_hash(previous, "previous")
    digest = hashlib.sha256()
    digest.update(_HASH_DOMAIN)
    digest.update(previous_bytes)
    digest.update(canonical(event))
    return digest.hexdigest().upper()


def build(events: Iterable[Mapping[str, object]]) -> list[dict[str, object]]:
    previous = GENESIS
    output: list[dict[str, object]] = []
    for index, raw_event in enumerate(events):
        if not isinstance(raw_event, Mapping):
            raise FlightError(f"event {index} must be an object")
        event = dict(raw_event)
        digest = event_hash(previous, event)
        output.append(
            {
                "scheme": SCHEME,
                "index": index,
                "prev_hash": previous,
                "event": event,
                "hash": digest,
            }
        )
        previous = digest
    return output


def verify(
    records: Iterable[object],
    *,
    require_non_empty: bool = True,
) -> tuple[bool, str]:
    previous = GENESIS
    seen = 0

    try:
        for expected_index, raw_record in enumerate(records):
            seen += 1
            if not isinstance(raw_record, dict):
                return False, "RECORD_NOT_OBJECT"

            if raw_record.get("scheme") != SCHEME:
                return False, "SCHEME"

            index = raw_record.get("index")
            if isinstance(index, bool) or not isinstance(index, int):
                return False, "INDEX_TYPE"
            if index != expected_index:
                return False, "INDEX"

            prev_hash = raw_record.get("prev_hash")
            try:
                _decode_hash(prev_hash, "prev_hash")
            except FlightError:
                return False, "PREVIOUS_HASH_FORMAT"
            if str(prev_hash).upper() != previous:
                return False, "PREVIOUS_HASH"

            raw_event = raw_record.get("event")
            if not isinstance(raw_event, dict):
                return False, "EVENT_TYPE"

            expected = event_hash(previous, raw_event)

            record_hash = raw_record.get("hash")
            try:
                _decode_hash(record_hash, "hash")
            except FlightError:
                return False, "HASH_FORMAT"
            if str(record_hash).upper() != expected:
                return False, "HASH"

            previous = expected
    except (FlightError, TypeError, ValueError, OverflowError):
        return False, "MALFORMED"

    if require_non_empty and seen == 0:
        return False, "EMPTY_CHAIN"
    return True, previous
