from __future__ import annotations

import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
C_SOURCE = ROOT / "firmware/OmniPkg/Applications/OmniProbe/OmniProbe.c"
INF = ROOT / "firmware/OmniPkg/Applications/OmniProbe/OmniProbe.inf"


class UefiPersistenceBarrierTests(unittest.TestCase):
    def test_omniprobe_declares_blockio_protocol(self) -> None:
        inf = INF.read_text(encoding="utf-8")
        assert "gEfiBlockIoProtocolGuid" in inf


    def test_persistent_files_apply_block_flush_barrier(self) -> None:
        source = C_SOURCE.read_text(encoding="utf-8")

        assert "#include <Protocol/BlockIo.h>" in source
        assert "FlushLoadedImageBlockDevice" in source
        assert "BlockIo->FlushBlocks (BlockIo)" in source
        assert "ApplyPersistenceBarrier" in source

        # SaveTraceStage, SaveEvidence and SaveDiag must all pass through the
        # persistence barrier after File->Flush, File->Close and Root->Close.
        assert source.count(
            "return ApplyPersistenceBarrier (ImageHandle, SystemTable, Status);"
        ) >= 3


if __name__ == "__main__":
    unittest.main()
