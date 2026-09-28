#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

LEGACY_TOOLCHAIN_SELECTION = 'export TARGET_TOOLS=`get_gcc_version "$CROSS_COMPILE"gcc`'
MODERN_TOOLCHAIN_SELECTION = 'export TARGET_TOOLS=GCC'


def patch_text(text: str) -> str:
    """Adapt pinned SCT 202509 to EDK II 202608's non-versioned GCC profile."""
    count = text.count(LEGACY_TOOLCHAIN_SELECTION)
    if count != 1:
        raise ValueError(
            "unexpected pinned SCT build.sh: legacy GCC selector occurrence "
            f"count is {count}, expected 1"
        )
    if MODERN_TOOLCHAIN_SELECTION in text:
        raise ValueError("SCT build.sh already contains the modern GCC selector")
    return text.replace(
        LEGACY_TOOLCHAIN_SELECTION,
        MODERN_TOOLCHAIN_SELECTION,
        1,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("build_sh")
    args = parser.parse_args()

    path = Path(args.build_sh)
    original = path.read_text(encoding="utf-8")
    patched = patch_text(original)
    path.write_text(patched, encoding="utf-8", newline="\n")

    check = path.read_text(encoding="utf-8")
    if check.count(MODERN_TOOLCHAIN_SELECTION) != 1:
        raise SystemExit("patched SCT build.sh does not contain exactly one GCC selector")
    if LEGACY_TOOLCHAIN_SELECTION in check:
        raise SystemExit("legacy GCC5-producing selector remains after patch")
    print("SCT_GCC_PROFILE_PATCH_PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
