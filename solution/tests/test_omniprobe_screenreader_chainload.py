from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "firmware/OmniPkg/Applications/OmniProbe/OmniProbe.c"


def _body(source: str, name: str) -> str:
    start = source.index(f"STATIC EFI_STATUS {name} (")
    return source[start: source.index("\n}\n", start)]


class OmniprobeScreenreaderChainloadTests(unittest.TestCase):
    def test_screen_reader_is_chainloaded_from_the_boot_media(self) -> None:
        source = SOURCE.read_text(encoding="utf-8")
        assert 'OMNI_SCREENREADER_FILE L"\\\\EFI\\\\OMNI\\\\SCREENREADER.EFI"' in source
        body = _body(source, "StartScreenReader")
        # Loaded from a bounded in-memory buffer, then given this key as its device.
        assert "OMNI_SCREENREADER_MAX_BYTES" in body
        assert "LoadImage (FALSE, ImageHandle, NULL, Buffer, Size, &Child)" in body
        assert "ChildImage->DeviceHandle = LoadedImage->DeviceHandle;" in body
        assert body.index("FreePool (Buffer)") < body.index("StartImage (Child")
        assert "return EFI_NOT_FOUND;" in body


    def test_evidence_is_persisted_before_the_reader_starts(self) -> None:
        source = SOURCE.read_text(encoding="utf-8")
        main = source[source.index("UefiMain ("):]
        start = main.index("StartScreenReader (ImageHandle, SystemTable)")
        assert main.index("SaveDiag (") < start
        assert main.index('"COMPLETE"') < start
        for marker in ("OMNI_SCREENREADER_ABSENT", "OMNI_SCREENREADER_FAIL", "OMNI_SCREENREADER_RETURNED"):
            assert marker in main
        assert start < main.index('WriteText ("OMNI_RETURN_TO_FIRMWARE\\n")')


if __name__ == "__main__":
    unittest.main()
