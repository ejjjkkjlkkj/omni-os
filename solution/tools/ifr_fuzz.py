#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import random
import struct
from pathlib import Path

from omni.ifr import IfrError, parse_hii_package_list


def valid_seed() -> bytes:
    data = bytearray(28)
    struct.pack_into("<I", data, 16, len(data))
    struct.pack_into("<I", data, 20, (0x02 << 24) | 8)
    data[24:28] = bytes((0x01, 0x82, 0x29, 0x02))
    return bytes(data)


def mutate(seed: bytes, rng: random.Random) -> bytes:
    data = bytearray(seed)
    edits = rng.randint(1, 12)
    for _ in range(edits):
        action = rng.randrange(4)
        if action == 0 and data:
            i = rng.randrange(len(data))
            data[i] ^= 1 << rng.randrange(8)
        elif action == 1 and len(data) < 4096:
            i = rng.randrange(len(data) + 1)
            data[i:i] = bytes((rng.randrange(256),))
        elif action == 2 and data:
            del data[rng.randrange(len(data))]
        elif data:
            i = rng.randrange(len(data))
            data[i] = rng.randrange(256)
    return bytes(data)


def arbitrary(rng: random.Random) -> bytes:
    size = rng.randrange(0, 2049)
    return rng.randbytes(size)


def validate_result(result: dict[str, object], source_length: int) -> None:
    declared = result.get("declared_length")
    packages = result.get("packages")
    if not isinstance(declared, int) or not 20 <= declared <= source_length:
        raise AssertionError("accepted result has invalid declared length")
    if not isinstance(packages, list):
        raise AssertionError("accepted result has invalid package list")

    previous = 19
    for package in packages:
        if not isinstance(package, dict):
            raise AssertionError("package is not a mapping")
        offset = package.get("offset")
        length = package.get("length")
        if not isinstance(offset, int) or not isinstance(length, int):
            raise AssertionError("package bounds are not integers")
        if offset <= previous or length < 4 or offset + length > declared:
            raise AssertionError("accepted package has invalid bounds/order")
        previous = offset

        if "ifr" in package:
            ifr = package["ifr"]
            if not isinstance(ifr, list):
                raise AssertionError("IFR result is not a list")
            last = -1
            for op in ifr:
                if not isinstance(op, dict):
                    raise AssertionError("IFR op is not a mapping")
                op_offset = op.get("offset")
                op_length = op.get("length")
                depth = op.get("depth")
                if not isinstance(op_offset, int) or not isinstance(op_length, int):
                    raise AssertionError("IFR op bounds are not integers")
                if op_offset <= last or op_length < 2:
                    raise AssertionError("IFR op did not make strict progress")
                if not isinstance(depth, int) or depth < 0:
                    raise AssertionError("IFR depth is invalid")
                last = op_offset


def run(cases: int, seed_value: int) -> dict[str, object]:
    rng = random.Random(seed_value)
    seed = valid_seed()
    accepted = 0
    rejected = 0

    for index in range(cases):
        if index % 3 == 0:
            payload = arbitrary(rng)
        elif index % 3 == 1:
            payload = mutate(seed, rng)
        else:
            data = bytearray(mutate(seed, rng))
            if len(data) >= 20:
                declared = rng.randrange(0, min(0x10000, len(data) + 128))
                data[16:20] = struct.pack("<I", declared)
            payload = bytes(data)

        try:
            result = parse_hii_package_list(payload)
        except IfrError:
            rejected += 1
        except Exception as exc:
            raise AssertionError(
                f"unexpected exception at case {index}: {type(exc).__name__}: {exc}"
            ) from exc
        else:
            validate_result(result, len(payload))
            accepted += 1

    return {
        "schema": "omniexec.ifr-fuzz.v1",
        "status": "PASS",
        "cases": cases,
        "seed": seed_value,
        "seed_sha256": hashlib.sha256(seed).hexdigest().upper(),
        "accepted": accepted,
        "rejected": rejected,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", type=int, default=100000)
    parser.add_argument("--seed", type=int, default=0x4F4D4E49)
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    if args.cases < 1:
        raise SystemExit("--cases must be >= 1")

    result = run(args.cases, args.seed)
    text = json.dumps(result, indent=2, sort_keys=True)
    print(text)
    if args.json_out:
        args.json_out.write_text(text + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
