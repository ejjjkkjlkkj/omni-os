from __future__ import annotations

from collections import Counter
from collections.abc import Iterable


class TraceError(ValueError):
    pass


NormalizedEvent = tuple[object, str, str, str, tuple[str, ...]]


def normalize(event: object) -> NormalizedEvent:
    if not isinstance(event, dict):
        raise TraceError("event must be an object")
    raw_node = event.get("node")
    if raw_node is None:
        raw_node = {}
    if not isinstance(raw_node, dict):
        raise TraceError("event node must be an object")
    raw_state = raw_node.get("state", [])
    if isinstance(raw_state, str):
        state = (raw_state.strip().lower(),) if raw_state.strip() else ()
    else:
        try:
            state = tuple(sorted(str(item).strip().lower() for item in raw_state))
        except TypeError as exc:
            raise TraceError("event state must be iterable") from exc
    kind = event.get("kind")
    # A normalized event must be hashable so consensus() can tally it; reject an
    # unhashable kind (list/dict/set) as an invalid event instead of letting it
    # crash the tally. diff() already treated other malformed input this way.
    try:
        hash(kind)
    except TypeError as exc:
        raise TraceError("event kind must be hashable") from exc
    return (
        kind,
        str(raw_node.get("role", "")).strip().lower(),
        " ".join(str(raw_node.get("name", "")).split()),
        " ".join(str(raw_node.get("value", "")).split()),
        state,
    )


def _normalize_trace(trace: Iterable[object]) -> list[NormalizedEvent]:
    if isinstance(trace, (str, bytes, dict)):
        raise TraceError("trace must be an iterable of event objects")
    return [normalize(event) for event in trace]


def diff(a: Iterable[object], b: Iterable[object]) -> dict[str, object]:
    try:
        aa = _normalize_trace(a)
        bb = _normalize_trace(b)
    except (TraceError, TypeError) as exc:
        return {
            "status": "FAIL",
            "reason": "INVALID_EVENT",
            "error": str(exc),
            "differences": [],
        }

    if not aa or not bb:
        return {
            "status": "FAIL",
            "reason": "EMPTY_TRACE",
            "a_events": len(aa),
            "b_events": len(bb),
            "differences": [],
        }

    differences: list[dict[str, object]] = []
    for index in range(max(len(aa), len(bb))):
        left = aa[index] if index < len(aa) else None
        right = bb[index] if index < len(bb) else None
        if left != right:
            differences.append({"index": index, "a": left, "b": right})

    return {
        "status": "PASS" if not differences else "DIVERGED",
        "a_events": len(aa),
        "b_events": len(bb),
        "differences": differences,
    }


def consensus(traces: Iterable[Iterable[object]]) -> dict[str, object]:
    try:
        trace_list = list(traces)
    except TypeError as exc:
        return {
            "status": "FAIL",
            "reason": "INVALID_TRACES",
            "error": str(exc),
            "traces": 0,
            "events": 0,
            "disagreements": [],
        }

    if not trace_list:
        return {
            "status": "FAIL",
            "reason": "NO_TRACES",
            "traces": 0,
            "events": 0,
            "disagreements": [],
        }
    if len(trace_list) < 2:
        return {
            "status": "FAIL",
            "reason": "INSUFFICIENT_TRACES",
            "traces": len(trace_list),
            "events": 0,
            "disagreements": [],
        }

    try:
        normalized = [_normalize_trace(trace) for trace in trace_list]
    except (TraceError, TypeError) as exc:
        return {
            "status": "FAIL",
            "reason": "INVALID_EVENT",
            "traces": len(trace_list),
            "events": 0,
            "error": str(exc),
            "disagreements": [],
        }

    empty = [index for index, trace in enumerate(normalized) if not trace]
    if empty:
        return {
            "status": "FAIL",
            "reason": "EMPTY_TRACE",
            "traces": len(normalized),
            "empty_trace_indexes": empty,
            "events": 0,
            "disagreements": [],
        }

    disagreements: list[dict[str, object]] = []
    unanimous = 0
    count = max(len(trace) for trace in normalized)
    for index in range(count):
        values = [trace[index] if index < len(trace) else None for trace in normalized]
        _, votes = Counter(values).most_common(1)[0]
        if votes == len(values):
            unanimous += 1
        else:
            disagreements.append(
                {
                    "index": index,
                    "values": values,
                    "majority_count": votes,
                }
            )

    return {
        "status": "PASS" if not disagreements else "DIVERGED",
        "traces": len(normalized),
        "events": count,
        "unanimous": unanimous,
        "disagreements": disagreements,
    }
