from __future__ import annotations

import unittest
from pathlib import Path

from tools.patch_sct_deprecated_protocols import (
    BLOCK_PATCHES,
    PATCHES,
    patch_block,
    patch_text,
)


class SctDeprecatedProtocolPatchTests(unittest.TestCase):
    def test_every_backport_line_is_removed_exactly_once(self) -> None:
        for path, expected_lines in PATCHES.items():
            source = "\r\n".join(("before", *expected_lines, "after", ""))
            result = patch_text(source, expected_lines, path)
            logical = {
                line.rstrip("\r\n")
                for line in result.splitlines(keepends=True)
            }
            for expected in expected_lines:
                self.assertNotIn(expected, logical)
            self.assertIn("before\r\n", result)
            self.assertIn("after\r\n", result)

    def test_missing_line_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            patch_text("before\n", ("required line",), "synthetic")

    def test_duplicate_line_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            patch_text("x\nx\n", ("x",), "synthetic")

    def test_upstream_unicode_stub_block_is_removed_exactly_once(self) -> None:
        path, expected_lines = next(iter(BLOCK_PATCHES.items()))
        source = "\r\n".join(("before", *expected_lines, "after", ""))
        result = patch_block(source, expected_lines, path)
        block = "\r\n".join(expected_lines)
        self.assertNotIn(block, result)
        self.assertIn("before\r\n", result)
        self.assertIn("after\r\n", result)

    def test_missing_block_fails_closed(self) -> None:
        path, expected_lines = next(iter(BLOCK_PATCHES.items()))
        with self.assertRaises(ValueError):
            patch_block("before\nafter\n", expected_lines, path)

    def test_duplicate_block_fails_closed(self) -> None:
        path, expected_lines = next(iter(BLOCK_PATCHES.items()))
        block = "\n".join(expected_lines)
        with self.assertRaises(ValueError):
            patch_block(block + "\n" + block, expected_lines, path)

    def test_pr362_unicode_stub_cleanup_is_covered(self) -> None:
        path = "TestInfrastructure/SCT/Framework/ENTS/EasLib/EntsLib.c"
        self.assertIn(path, BLOCK_PATCHES)
        block = "\n".join(BLOCK_PATCHES[path])
        self.assertIn("EntsLibStubUnicodeInterface", block)
        self.assertIn("EntsUnicodeInterface", block)

    def test_both_sct_workflows_apply_deprecated_protocol_patch(self) -> None:
        command = "python3 tools/patch_sct_deprecated_protocols.py SctPkg"
        for path in (
            Path(".github/workflows/uefi-sct-build.yml"),
            Path(".github/workflows/uefi-sct-runtime.yml"),
        ):
            text = path.read_text(encoding="utf-8")
            self.assertIn(command, text)
            self.assertLess(
                text.index("python3 tools/patch_sct_efi_header.py SctPkg/Include/Efi.h"),
                text.index(command),
            )


if __name__ == "__main__":
    unittest.main()
