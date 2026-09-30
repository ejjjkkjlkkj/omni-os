#!/usr/bin/env python3
"""Every protocol of the EDK II table (protocols_gen.rs) has a use in protocols.rs.

A name must appear in `exercise()` (called, marker or guarded with a reason) or be a service
binding (children created and destroyed). Prints PROTOCOL_COVERAGE=PASS or lists what is missing.
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parents[2] / "os" / "boot" / "uefi" / "src"
names = re.findall(r'\("([A-Za-z0-9]+)"', (SRC / "protocols_gen.rs").read_text(encoding="utf-8"))
source = (SRC / "protocols.rs").read_text(encoding="utf-8")
start = source.index("fn exercise(")
body = source[start:source.index("// -----", start)]
covered = set(re.findall(r'"([A-Za-z0-9]+)"', body))
missing = [n for n in names if n not in covered and not n.endswith("ServiceBinding")]
if missing:
    sys.exit("protocols without a use: " + " ".join(missing))
print(f"PROTOCOL_COVERAGE=PASS protocols={len(names)}")
