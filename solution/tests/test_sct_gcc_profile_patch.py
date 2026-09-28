from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.patch_sct_gcc_profile import (
    LEGACY_TOOLCHAIN_SELECTION,
    MODERN_TOOLCHAIN_SELECTION,
    patch_text,
)


class SctGccProfilePatchTests(unittest.TestCase):
    def test_pinned_legacy_selector_becomes_modern_gcc(self) -> None:
        source = "before\n" + LEGACY_TOOLCHAIN_SELECTION + "\nafter\n"
        result = patch_text(source)

        self.assertNotIn(LEGACY_TOOLCHAIN_SELECTION, result)
        self.assertEqual(result.count(MODERN_TOOLCHAIN_SELECTION), 1)
        self.assertIn("before\n", result)
        self.assertIn("\nafter\n", result)

    def test_missing_or_drifted_selector_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            patch_text("export TARGET_TOOLS=GCC5\n")

    def test_duplicate_selector_fails_closed(self) -> None:
        text = LEGACY_TOOLCHAIN_SELECTION + "\n" + LEGACY_TOOLCHAIN_SELECTION
        with self.assertRaises(ValueError):
            patch_text(text)

    def test_sct_build_checks_out_repository_before_patch_helper(self) -> None:
        path = Path(".github/workflows/uefi-sct-build.yml")
        text = path.read_text(encoding="utf-8")
        checkout = "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        patch = "python3 tools/patch_sct_gcc_profile.py SctPkg/build.sh"
        self.assertIn(checkout, text)
        self.assertIn(patch, text)
        self.assertLess(text.index(checkout), text.index(patch))

    def test_workflows_use_modern_sct_output_directory(self) -> None:
        for path in (
            Path(".github/workflows/uefi-sct-build.yml"),
            Path(".github/workflows/uefi-sct-runtime.yml"),
        ):
            text = path.read_text(encoding="utf-8")
            self.assertIn("python3 tools/patch_sct_gcc_profile.py SctPkg/build.sh", text)
            self.assertIn("Build/UefiSct/RELEASE_GCC/SctPackageX64", text)
            self.assertNotIn("RELEASE_GCC5", text)


if __name__ == "__main__":
    unittest.main()
