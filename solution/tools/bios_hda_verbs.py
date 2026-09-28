"""Extract HDA pin-configuration verb tables from a firmware image (read-only).

Firmware programs each codec pin's Configuration Default with four verbs
0x71C..0x71F (one byte each). In the image they appear as consecutive
little-endian DWORDs (cad<<28 | nid<<20 | verb<<8 | byte). This scanner finds
those groups, rebuilds the 32-bit config per pin and decodes device/location/
connectivity so the UEFI probe can target the real speaker pin.

python tools/bios_hda_verbs.py <image> [--json out.json]
"""
from __future__ import annotations

import argparse
import json
import struct
from pathlib import Path

DEVICES = ["Line Out", "Speaker", "HP Out", "CD", "SPDIF Out", "Digital Other Out", "Modem Line", "Modem Handset",
           "Line In", "AUX", "Mic In", "Telephony", "SPDIF In", "Digital Other In", "Reserved", "Other"]
CONNECTIVITY = ["Jack", "None", "Fixed (internal)", "Jack + Internal"]


def decode(cfg: int) -> dict:
    return {
        "config": f"0x{cfg:08X}",
        "connectivity": CONNECTIVITY[cfg >> 30],
        "location": (cfg >> 24) & 0x3F,
        "device": DEVICES[(cfg >> 20) & 0xF],
        "connection_type": (cfg >> 16) & 0xF,
        "color": (cfg >> 12) & 0xF,
        "misc": (cfg >> 8) & 0xF,
        "association": (cfg >> 4) & 0xF,
        "sequence": cfg & 0xF,
    }


def scan(data: bytes) -> list[dict]:
    tables = []
    for align in range(4):
        words = [w for (w,) in struct.iter_unpack("<I", data[align: align + (len(data) - align) // 4 * 4])]
        i = 0
        while i < len(words) - 3:
            w = words[i]
            if (w >> 8) & 0xFFF == 0x71C:
                cad, nid = w >> 28, (w >> 20) & 0xFF
                group = words[i:i + 4]
                if all(((g >> 28) == cad and ((g >> 20) & 0xFF) == nid and ((g >> 8) & 0xFFF) == 0x71C + k) for k, g in enumerate(group)):
                    # Collect a run of consecutive pin groups for the same codec address.
                    start = i
                    pins = []
                    while i < len(words) - 3:
                        grp = words[i:i + 4]
                        n = (grp[0] >> 20) & 0xFF
                        if not all(((g >> 28) == cad and ((g >> 20) & 0xFF) == n and ((g >> 8) & 0xFFF) == 0x71C + k) for k, g in enumerate(grp)):
                            break
                        cfg = sum((g & 0xFF) << (8 * k) for k, g in enumerate(grp))
                        pins.append({"nid": f"0x{n:02X}", **decode(cfg)})
                        i += 4
                    if len(pins) >= 3:
                        offset = align + start * 4
                        vendor = words[start - 1] if start >= 1 else 0  # tables usually lead with the codec ID
                        tables.append({"offset": f"0x{offset:X}", "codec_address": cad,
                                       "preceding_dword": f"0x{vendor:08X}", "pins": pins})
                    continue
            i += 1
    uniq, seen = [], set()
    for t in tables:
        key = tuple(p["config"] + p["nid"] for p in t["pins"])
        if key not in seen:
            seen.add(key)
            uniq.append(t)
    return uniq


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("image", type=Path)
    ap.add_argument("--json", type=Path)
    a = ap.parse_args()
    tables = scan(a.image.read_bytes())
    for t in tables:
        print(f"table @ {t['offset']} cad={t['codec_address']} preceded by {t['preceding_dword']}")
        for p in t["pins"]:
            if p["connectivity"] != "None":
                print(f"   nid {p['nid']} {p['config']} {p['device']:<12} {p['connectivity']:<17} loc=0x{p['location']:02X} assoc={p['association']} seq={p['sequence']}")
    if a.json:
        a.json.write_text(json.dumps(tables, indent=1), encoding="utf-8")
    return 0 if tables else 1


if __name__ == "__main__":
    raise SystemExit(main())
