from __future__ import annotations

import unittest
from pathlib import Path

from tools.patch_sct_efi_header import LEGACY_GUARD, MODERN_GUARD, patch_text


class SctEfiHeaderPatchTests(unittest.TestCase):
    def _legacy(self, newline: str = "\n") -> str:
        return newline.join(
            (
                "before",
                LEGACY_GUARD,
                '  #include "Tiano.h"',
                '  #include "PeiHob.h"',
                "after",
                "",
            )
        )

    def test_legacy_base_h_guard_becomes_modern_cpu_guard(self) -> None:
        result = patch_text(self._legacy())
        self.assertNotIn(LEGACY_GUARD + "\n", result)
        self.assertEqual(result.count(MODERN_GUARD), 1)
        self.assertIn('  #include "Tiano.h"', result)
        self.assertIn('  #include "PeiHob.h"', result)

    def test_crlf_is_preserved(self) -> None:
        source = self._legacy("\r\n")
        result = patch_text(source)
        self.assertIn(MODERN_GUARD + "\r\n", result)
        self.assertNotIn(MODERN_GUARD + "\n", result.replace("\r\n", ""))

    def test_missing_or_drifted_guard_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            patch_text('#if defined(SOMETHING_ELSE)\n  #include "Tiano.h"\n')

    def test_duplicate_legacy_context_fails_closed(self) -> None:
        context = "\n".join(
            (
                LEGACY_GUARD,
                '  #include "Tiano.h"',
                '  #include "PeiHob.h"',
            )
        )
        with self.assertRaises(ValueError):
            patch_text(context + "\n" + context + "\n")

    def test_both_sct_workflows_apply_modern_edk2_patch(self) -> None:
        command = "python3 tools/patch_sct_efi_header.py SctPkg/Include/Efi.h"
        for path in (
            Path(".github/workflows/uefi-sct-build.yml"),
            Path(".github/workflows/uefi-sct-runtime.yml"),
        ):
            text = path.read_text(encoding="utf-8")
            self.assertIn(command, text)
            self.assertLess(
                text.index("python3 tools/patch_sct_gcc_profile.py SctPkg/build.sh"),
                text.index(command),
            )


if __name__ == "__main__":
    unittest.main()
