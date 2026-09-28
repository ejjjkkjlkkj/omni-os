"""Static inventory of HII form sets in a firmware image (read-only).

Decompresses the image's LZMA GUID-defined sections, finds HII FORMS packages
(type 0x02 starting with FORM_SET), parses them with omni.ifr and compares the
totals with what OmniProbe observed at runtime (OMNI-EVIDENCE.TXT), which only
contains the form sets the firmware publishes to a boot application.

python tools/bios_ifr_inventory.py <image> [--evidence OMNI-EVIDENCE.TXT] [--json out.json]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import pathlib
import sys
import uuid

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from omni.ifr import QUESTION_OPS, IfrError, parse_ifr_stream  # noqa: E402

LZMA_GUID = uuid.UUID("EE4E5898-3914-4259-9D6E-DC7BD79403CF").bytes_le
KNOWN = {"7b59104a-c00d-4158-87ff-f04d6396a915": "AMI Aptio Setup (main setup, entered with F2/Del)"}


def sections(image: bytes) -> list[bytes]:
    out = [image]

    def walk(buf: bytes, depth: int) -> None:
        pos = 0
        while (i := buf.find(LZMA_GUID, pos)) >= 0:
            pos = i + 16
            if i < 4 or buf[i - 1] != 0x02:  # EFI_SECTION_GUID_DEFINED
                continue
            size = int.from_bytes(buf[i - 4:i - 1], "little")
            offset = int.from_bytes(buf[i + 16:i + 18], "little")
            try:
                data = lzma.LZMADecompressor(format=lzma.FORMAT_ALONE).decompress(buf[i - 4 + offset:i - 4 + size])
            except lzma.LZMAError:
                continue
            out.append(data)
            if depth < 3:
                walk(data, depth + 1)

    walk(image, 0)
    return out


def formsets(image: bytes) -> list[dict]:
    found, seen = [], set()
    for blob in sections(image):
        for i in range(len(blob) - 6):
            if blob[i + 3] != 0x02 or blob[i + 4] != 0x0E or not blob[i + 5] & 0x80:
                continue
            length = int.from_bytes(blob[i:i + 3], "little")
            if not 24 < length < 0x100000 or i + length > len(blob):
                continue
            body = blob[i + 4:i + length]
            if body in seen:
                continue
            try:
                ops = parse_ifr_stream(body)
            except IfrError:
                continue
            seen.add(body)
            guid = str(uuid.UUID(bytes_le=body[2:18]))
            found.append({"guid": guid, "name": KNOWN.get(guid, ""), "opcodes": len(ops),
                          "questions": sum(o.opcode in QUESTION_OPS for o in ops),
                          "passwords": sum(o.password for o in ops)})
    return sorted(found, key=lambda f: -f["questions"])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("image", type=pathlib.Path)
    ap.add_argument("--evidence", type=pathlib.Path)
    ap.add_argument("--json", type=pathlib.Path)
    a = ap.parse_args()
    raw = a.image.read_bytes()
    sets = formsets(raw)
    report = {"image": a.image.name, "sha256": hashlib.sha256(raw).hexdigest(), "formsets": sets,
              "totals": {"formsets": len(sets), "questions": sum(s["questions"] for s in sets),
                         "passwords": sum(s["passwords"] for s in sets)}}
    if a.evidence and a.evidence.exists():
        runtime = dict(line.split("=", 1) for line in a.evidence.read_text().splitlines() if "=" in line)
        report["runtime"] = {k: int(runtime[k]) for k in ("OMNI_HII_FORM_PACKAGES", "OMNI_HII_QUESTIONS", "OMNI_HII_PASSWORDS") if k in runtime}
    print(json.dumps(report, indent=1))
    if a.json:
        a.json.write_text(json.dumps(report, indent=1), encoding="utf-8")
    return 0 if sets else 1


if __name__ == "__main__":
    raise SystemExit(main())
