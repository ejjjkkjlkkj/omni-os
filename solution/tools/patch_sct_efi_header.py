#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

LEGACY_GUARD = '#ifndef __BASE_H__'
MODERN_GUARD = (
    '#if !defined(__BASE_H__) && !defined(MDE_CPU_IA32) && '
    '!defined(MDE_CPU_X64) && !defined(MDE_CPU_EBC) && '
    '!defined(MDE_CPU_ARM) && !defined(MDE_CPU_AARCH64) && '
    '!defined(MDE_CPU_RISCV64) && !defined(MDE_CPU_LOONGARCH64)'
)


def patch_text(text: str) -> str:
    """Backport the modern EDK II Base.h detection used by upstream SCT."""
    newline = "\r\n" if "\r\n" in text else "\n"
    legacy_context = newline.join(
        (
            LEGACY_GUARD,
            '  #include "Tiano.h"',
            '  #include "PeiHob.h"',
        )
    )
    modern_context = newline.join(
        (
            MODERN_GUARD,
            '  #include "Tiano.h"',
            '  #include "PeiHob.h"',
        )
    )

    count = text.count(legacy_context)
    if count != 1:
        raise ValueError(
            "unexpected pinned SCT Efi.h: legacy Base.h guard context "
            f"count is {count}, expected 1"
        )
    if MODERN_GUARD in text:
        raise ValueError("SCT Efi.h already contains the modern Base.h guard")
    return text.replace(legacy_context, modern_context, 1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("efi_h")
    args = parser.parse_args()

    path = Path(args.efi_h)
    with path.open("r", encoding="utf-8", newline="") as stream:
        original = stream.read()
    patched = patch_text(original)
    with path.open("w", encoding="utf-8", newline="") as stream:
        stream.write(patched)

    with path.open("r", encoding="utf-8", newline="") as stream:
        check = stream.read()
    if check.count(MODERN_GUARD) != 1:
        raise SystemExit("patched SCT Efi.h does not contain exactly one modern guard")
    if LEGACY_GUARD + ("\r\n" if "\r\n" in check else "\n") in check:
        raise SystemExit("legacy __BASE_H__-only guard remains after patch")
    print("SCT_EDK2_BASE_H_PATCH_PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
