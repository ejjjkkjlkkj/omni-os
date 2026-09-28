#!/usr/bin/env python3
"""Fail-closed Intel HDA connection-list parser and output-route planner.

This is build/test tooling for the first-party UEFI HDA path. It does not claim
physical firmware execution. It mirrors HDA connection-list encoding, expands
range entries, searches a bounded upstream graph, and emits selector indices.
"""
from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from typing import Mapping, Sequence

GET_PARAMETER = 0xF00
GET_CONNECTION_SELECT = 0xF01
GET_CONNECTION_LIST_ENTRY = 0xF02
SET_CONNECTION_SELECT = 0x701

WIDGET_AUDIO_OUTPUT = 0x0
WIDGET_AUDIO_INPUT = 0x1
WIDGET_MIXER = 0x2
WIDGET_SELECTOR = 0x3
WIDGET_PIN = 0x4
WIDGET_POWER = 0x5
WIDGET_VOLUME_KNOB = 0x6
WIDGET_BEEP = 0x7
WIDGET_VENDOR = 0xF

_TRAVERSABLE_TYPES = {
    WIDGET_MIXER,
    WIDGET_SELECTOR,
    WIDGET_POWER,
    WIDGET_VOLUME_KNOB,
    WIDGET_VENDOR,
}
_SELECTABLE_TYPES = {
    WIDGET_AUDIO_INPUT,
    WIDGET_SELECTOR,
    WIDGET_PIN,
    WIDGET_VENDOR,
}


class HdaRouteError(ValueError):
    pass


@dataclass(frozen=True)
class RawConnection:
    nid: int
    range_end: bool = False


@dataclass(frozen=True)
class RoutePlan:
    path: tuple[int, ...]
    selectors: tuple[tuple[int, int], ...]


def _validate_nid(nid: int, mask: int) -> None:
    if not isinstance(nid, int) or nid <= 0 or nid > mask:
        raise HdaRouteError(f"invalid NID 0x{nid:x}")


def decode_raw_connection_entries(
    length_parameter: int,
    responses: Mapping[int, int],
) -> tuple[RawConnection, ...]:
    """Decode F02h responses using the Connection List Length parameter."""
    if not 0 <= length_parameter <= 0xFF:
        raise HdaRouteError("connection-list length parameter must fit in 8 bits")
    count = length_parameter & 0x7F
    if count == 0:
        return ()

    long_form = bool(length_parameter & 0x80)
    width = 16 if long_form else 8
    per_response = 2 if long_form else 4
    mask = (1 << (width - 1)) - 1
    range_bit = 1 << (width - 1)

    raw: list[RawConnection] = []
    for base in range(0, count, per_response):
        if base not in responses:
            raise HdaRouteError(f"missing F02h response for index {base}")
        response = responses[base]
        if not 0 <= response <= 0xFFFFFFFF:
            raise HdaRouteError("F02h response must fit in 32 bits")
        for slot in range(per_response):
            if len(raw) == count:
                break
            value = (response >> (slot * width)) & ((1 << width) - 1)
            nid = value & mask
            is_range = bool(value & range_bit)
            _validate_nid(nid, mask)
            if is_range and not raw:
                raise HdaRouteError("first connection entry cannot terminate a range")
            if is_range and raw[-1].range_end:
                raise HdaRouteError("consecutive range-end entries are invalid")
            raw.append(RawConnection(nid=nid, range_end=is_range))
    return tuple(raw)


def expand_connection_entries(raw: Sequence[RawConnection]) -> tuple[int, ...]:
    """Expand HDA range entries into the logical ordered connection list."""
    out: list[int] = []
    previous_raw_nid: int | None = None
    previous_was_range = False
    for index, entry in enumerate(raw):
        if entry.nid <= 0:
            raise HdaRouteError(f"invalid zero NID at entry {index}")
        if entry.range_end:
            if previous_raw_nid is None:
                raise HdaRouteError("range end has no previous entry")
            if previous_was_range:
                raise HdaRouteError("consecutive range-end entries are invalid")
            if previous_raw_nid >= entry.nid:
                raise HdaRouteError(
                    f"descending or empty NID range 0x{previous_raw_nid:x}..0x{entry.nid:x}"
                )
            out.extend(range(previous_raw_nid + 1, entry.nid + 1))
        else:
            out.append(entry.nid)
        previous_was_range = entry.range_end
        previous_raw_nid = entry.nid
    return tuple(out)


def decode_connection_list(
    length_parameter: int,
    responses: Mapping[int, int],
) -> tuple[int, ...]:
    return expand_connection_entries(
        decode_raw_connection_entries(length_parameter, responses)
    )


def find_output_route(
    pin_nid: int,
    widget_types: Mapping[int, int],
    connections: Mapping[int, Sequence[int]],
    *,
    max_depth: int = 16,
) -> tuple[int, ...]:
    """Find the shortest stable pin-to-Audio-Output route."""
    if max_depth < 1:
        raise HdaRouteError("max_depth must be positive")
    if widget_types.get(pin_nid) != WIDGET_PIN:
        raise HdaRouteError(f"NID 0x{pin_nid:x} is not a Pin Complex")

    queue: deque[tuple[int, ...]] = deque([(pin_nid,)])
    visited_depth: dict[int, int] = {pin_nid: 0}

    while queue:
        path = queue.popleft()
        node = path[-1]
        depth = len(path) - 1
        if depth >= max_depth:
            continue

        for upstream in connections.get(node, ()):
            if upstream in path:
                continue
            kind = widget_types.get(upstream)
            if kind is None:
                continue
            candidate = path + (upstream,)
            if kind == WIDGET_AUDIO_OUTPUT:
                return candidate
            if kind not in _TRAVERSABLE_TYPES:
                continue
            next_depth = depth + 1
            old = visited_depth.get(upstream)
            if old is not None and old <= next_depth:
                continue
            visited_depth[upstream] = next_depth
            queue.append(candidate)

    raise HdaRouteError(f"no safe Audio Output route from pin 0x{pin_nid:x}")


def build_route_plan(
    pin_nid: int,
    widget_types: Mapping[int, int],
    connections: Mapping[int, Sequence[int]],
    *,
    max_depth: int = 16,
) -> RoutePlan:
    """Resolve a route and derive required Set Connection Select writes."""
    path = find_output_route(
        pin_nid,
        widget_types,
        connections,
        max_depth=max_depth,
    )
    selectors: list[tuple[int, int]] = []
    for node, upstream in zip(path, path[1:]):
        entries = tuple(connections.get(node, ()))
        if upstream not in entries:
            raise HdaRouteError(
                f"path edge 0x{node:x}->0x{upstream:x} missing from connection list"
            )
        if len(entries) <= 1:
            continue
        kind = widget_types[node]
        if kind == WIDGET_MIXER:
            continue
        if kind not in _SELECTABLE_TYPES:
            raise HdaRouteError(
                f"multi-input widget 0x{node:x} type {kind} has no safe selector policy"
            )
        selectors.append((node, entries.index(upstream)))
    return RoutePlan(path=path, selectors=tuple(selectors))


def encode_set_connection_select(codec_address: int, nid: int, index: int) -> int:
    """Encode a 32-bit Set Connection Select Control command."""
    if not 0 <= codec_address <= 0xF:
        raise HdaRouteError("codec address out of range")
    if not 0 <= nid <= 0xFF:
        raise HdaRouteError("command NID out of range")
    if not 0 <= index <= 0xFF:
        raise HdaRouteError("connection index out of range")
    return (
        (codec_address << 28)
        | (nid << 20)
        | (SET_CONNECTION_SELECT << 8)
        | index
    )
