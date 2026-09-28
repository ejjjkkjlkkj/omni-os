from __future__ import annotations

import hashlib
import pathlib
import struct

FV_SIGNATURE = b"_FVH"
FV_SIGNATURE_OFFSET = 40
FV_FIXED_HEADER_SIZE = 56
FV_CHECKSUM_OFFSET = 50

MARKERS = (
    b"AGESA",
    b"PSP",
    b"SMM",
    b"SecureBoot",
    b"AMITSESetup",
    b"AsusSetupIoInterface",
)


def _offsets(data: bytes, needle: bytes) -> list[int]:
    result: list[int] = []
    start = 0
    while True:
        index = data.find(needle, start)
        if index < 0:
            return result
        result.append(index)
        start = index + 1


def _checksum16_zero(header: bytes) -> bool:
    if not header or len(header) % 2:
        return False
    total = sum(
        struct.unpack_from("<H", header, offset)[0]
        for offset in range(0, len(header), 2)
    )
    return (total & 0xFFFF) == 0


def _block_map_terminator(header: bytes) -> bool:
    """Require a {NumBlocks=0, Length=0} terminator in the block-map area."""

    if len(header) < FV_FIXED_HEADER_SIZE + 8:
        return False
    for offset in range(FV_FIXED_HEADER_SIZE, len(header) - 7, 8):
        blocks, block_length = struct.unpack_from("<II", header, offset)
        if blocks == 0 and block_length == 0:
            return True
        if blocks == 0 or block_length == 0:
            return False
    return False


def inspect(path: str | pathlib.Path) -> dict[str, object]:
    source = pathlib.Path(path)
    data = source.read_bytes()
    volumes: list[dict[str, object]] = []

    for signature_offset in _offsets(data, FV_SIGNATURE):
        base = signature_offset - FV_SIGNATURE_OFFSET
        if base < 0 or base + FV_FIXED_HEADER_SIZE > len(data):
            continue

        length = struct.unpack_from("<Q", data, base + 32)[0]
        attributes = struct.unpack_from("<I", data, base + 44)[0]
        header_length = struct.unpack_from("<H", data, base + 48)[0]
        stored_checksum = struct.unpack_from("<H", data, base + FV_CHECKSUM_OFFSET)[0]
        ext_header_offset = struct.unpack_from("<H", data, base + 52)[0]
        revision = data[base + 55]

        valid_bounds = (
            length >= header_length >= FV_FIXED_HEADER_SIZE
            and base + length <= len(data)
            and base + header_length <= len(data)
        )
        header_even = header_length % 2 == 0
        header = (
            data[base : base + header_length]
            if valid_bounds and header_even
            else b""
        )
        checksum_valid = bool(header) and _checksum16_zero(header)
        block_map_terminated = bool(header) and _block_map_terminator(header)
        ext_header_valid = (
            ext_header_offset == 0
            or (
                ext_header_offset >= FV_FIXED_HEADER_SIZE
                and ext_header_offset < length
            )
        )

        volumes.append(
            {
                "base": base,
                "length": length,
                "attributes": attributes,
                "header_length": header_length,
                "stored_checksum": stored_checksum,
                "ext_header_offset": ext_header_offset,
                "revision": revision,
                "valid_bounds": valid_bounds,
                "header_even": header_even,
                "header_checksum_valid": checksum_valid,
                "block_map_terminated": block_map_terminated,
                "ext_header_valid": ext_header_valid,
                "valid_header": (
                    valid_bounds
                    and header_even
                    and checksum_valid
                    and block_map_terminated
                    and ext_header_valid
                ),
            }
        )

    marker_map = {marker.decode(): _offsets(data.lower(), marker.lower()) for marker in MARKERS}
    return {
        "path": str(source),
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest().upper(),
        "firmware_volumes": volumes,
        "markers": {key: value for key, value in marker_map.items() if value},
        "mutation": False,
    }
