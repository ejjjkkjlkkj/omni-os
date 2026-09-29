#!/usr/bin/env python3
"""The on-disk boot-state record (os/crates/aw-bootstate/src/record.rs), for tests and tooling.

    bootstate.py encode OUT SEQ SEL_GEN SEL_KERNEL PREV_GEN PREV_KERNEL FLOOR STATE TRIES
        STATE: trial | attempt | successful; *_KERNEL: the image whose SHA-256 names the generation
    bootstate.py decode FILE        prints the record, or fails if it is not exactly valid
"""
from __future__ import annotations

import hashlib
import struct
import sys
import zlib
from pathlib import Path

MAGIC = b"OMNIBST\x01"
STATES = {"trial": 0, "attempt": 1, "successful": 2}
NAMES = {v: k for k, v in STATES.items()}


def encode(seq: int, sel_gen: int, sel_digest: bytes, prev_gen: int, prev_digest: bytes,
           floor: int, state: str, tries: int) -> bytes:
    body = bytearray(124)
    body[0:8] = MAGIC
    struct.pack_into("<H", body, 8, 1)
    struct.pack_into("<Q", body, 12, seq)
    struct.pack_into("<Q", body, 20, sel_gen)
    body[28:60] = sel_digest
    struct.pack_into("<Q", body, 60, prev_gen)
    body[68:100] = prev_digest
    struct.pack_into("<Q", body, 100, floor)
    body[108] = STATES[state]
    body[109] = tries
    return bytes(body) + struct.pack("<I", zlib.crc32(bytes(body)))


def decode(data: bytes) -> dict:
    if len(data) != 128 or data[:8] != MAGIC:
        raise ValueError("not a boot-state record")
    if struct.unpack_from("<I", data, 124)[0] != zlib.crc32(data[:124]):
        raise ValueError("bad checksum")
    return {
        "sequence": struct.unpack_from("<Q", data, 12)[0],
        "selected": struct.unpack_from("<Q", data, 20)[0],
        "known_good": struct.unpack_from("<Q", data, 60)[0],
        "rollback_floor": struct.unpack_from("<Q", data, 100)[0],
        "state": NAMES.get(data[108], "invalid"),
        "tries": data[109],
    }


def main(argv: list[str]) -> int:
    if len(argv) >= 11 and argv[1] == "encode":
        digest = lambda path: hashlib.sha256(Path(path).read_bytes()).digest()
        record = encode(int(argv[3]), int(argv[4]), digest(argv[5]), int(argv[6]), digest(argv[7]),
                        int(argv[8]), argv[9], int(argv[10]))
        Path(argv[2]).write_bytes(record)
        return 0
    if len(argv) == 3 and argv[1] == "decode":
        record = decode(Path(argv[2]).read_bytes())
        print(" ".join(f"{k}={v}" for k, v in record.items()))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
