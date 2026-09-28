#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

PATCHES: dict[str, tuple[str, ...]] = {
    "Include/Library/EntsLib.h": (
        "#include EFI_PROTOCOL_DEFINITION (DeviceIo)",
        "extern EFI_GUID             gtEfiDeviceIoProtocolGuid;",
        "extern EFI_GUID             gtEfiUnicodeCollationProtocolGuid;",
    ),
    "Library/SctLib/Guid.c": (
        "#include EFI_PROTOCOL_DEFINITION (DeviceIo)",
        '  { &gEfiDeviceIoProtocolGuid,          L"DevIo" },',
    ),
    "Library/SctLib/SctLib.inf": (
        "  gEfiDeviceIoProtocolGuid",
    ),
    "TestCase/UEFI/EFI/Generic/EfiCompliant/BlackBoxTest/EfiCompliantBBTestPlatform_uefi.c": (
        "#include EFI_PROTOCOL_DEFINITION (Ip4Config)",
    ),
    "TestCase/UEFI/EFI/Generic/EfiCompliant/BlackBoxTest/EfiCompliantBBTest_uefi.inf": (
        "  gEfiIp4ConfigProtocolGuid",
    ),
    "TestInfrastructure/SCT/Framework/ENTS/EasLib/EntsLib.c": (
        "EFI_GUID                        gtEfiDeviceIoProtocolGuid         = EFI_DEVICE_IO_PROTOCOL_GUID;",
        "EFI_GUID                        gtEfiUnicodeCollationProtocolGuid = EFI_UNICODE_COLLATION_PROTOCOL_GUID;",
    ),
    "TestInfrastructure/SCT/Framework/ENTS/EasLib/EntsGuid.c": (
        '  { &gtEfiDeviceIoProtocolGuid, L"DevIo"},',
        '  { &gtEfiUnicodeCollationProtocolGuid, L"UnicodeCollation"},',
    ),
}

BLOCK_PATCHES: dict[str, tuple[str, ...]] = {
    "TestInfrastructure/SCT/Framework/ENTS/EasLib/EntsLib.c": (
        "//",
        "// Unicode collation functions that are in use",
        "//",
        "EFI_UNICODE_COLLATION_PROTOCOL  EntsLibStubUnicodeInterface = {",
        "  EntsLibStubStriCmp,",
        "  EntsLibStubMetaiMatch,",
        "  EntsLibStubStrLwrUpr,",
        "  EntsLibStubStrLwrUpr,",
        "  NULL, // FatToStr",
        "  NULL, // StrToFat",
        "  NULL  // SupportedLanguages",
        "};",
        "",
        "EFI_UNICODE_COLLATION_PROTOCOL  *EntsUnicodeInterface = &EntsLibStubUnicodeInterface;",
    ),
}


def patch_text(text: str, expected_lines: tuple[str, ...], label: str) -> str:
    lines = text.splitlines(keepends=True)
    for expected in expected_lines:
        matches = [
            index
            for index, line in enumerate(lines)
            if line.rstrip("\r\n") == expected
        ]
        if len(matches) != 1:
            raise ValueError(
                f"unexpected pinned SCT {label}: line {expected!r} "
                f"occurrence count is {len(matches)}, expected 1"
            )
        del lines[matches[0]]
    return "".join(lines)


def patch_block(text: str, expected_lines: tuple[str, ...], label: str) -> str:
    newline = "\r\n" if "\r\n" in text else "\n"
    block = newline.join(expected_lines)
    count = text.count(block)
    if count != 1:
        raise ValueError(
            f"unexpected pinned SCT {label}: block occurrence count is "
            f"{count}, expected 1"
        )
    return text.replace(block, "", 1)


def _read(path: Path) -> str:
    with path.open("r", encoding="utf-8", newline="") as stream:
        return stream.read()


def _write(path: Path, text: str) -> None:
    with path.open("w", encoding="utf-8", newline="") as stream:
        stream.write(text)


def patch_tree(root: Path) -> None:
    for relative, expected_lines in PATCHES.items():
        path = root / relative
        if not path.is_file():
            raise ValueError(f"missing pinned SCT file: {relative}")
        patched = patch_text(_read(path), expected_lines, relative)
        _write(path, patched)
        logical = {
            line.rstrip("\r\n")
            for line in _read(path).splitlines(keepends=True)
        }
        stale = [line for line in expected_lines if line in logical]
        if stale:
            raise SystemExit(
                f"deprecated SCT references remain in {relative}: {stale}"
            )

    for relative, expected_lines in BLOCK_PATCHES.items():
        path = root / relative
        if not path.is_file():
            raise ValueError(f"missing pinned SCT file: {relative}")
        patched = patch_block(_read(path), expected_lines, relative)
        _write(path, patched)

        newline = "\r\n" if "\r\n" in patched else "\n"
        stale_block = newline.join(expected_lines)
        if stale_block in _read(path):
            raise SystemExit(
                f"deprecated SCT block remains in {relative}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("sct_pkg")
    args = parser.parse_args()
    patch_tree(Path(args.sct_pkg))
    print("SCT_DEPRECATED_PROTOCOL_PATCH_PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
