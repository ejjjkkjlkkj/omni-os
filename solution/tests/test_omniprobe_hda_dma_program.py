from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "firmware/OmniPkg/Applications/OmniProbe/OmniProbe.c"

class OmniprobeHdaDmaProgramTests(unittest.TestCase):
    def test_hda_dma_program_and_run_proof_is_present(self) -> None:
        source = SOURCE.read_text(encoding="utf-8")
        required = [
            "ProgramHdaDmaProof",
            "EfiPciIoOperationBusMasterCommonBuffer",
            "OMNI_HDA_DMA_FORMAT",
            "OMNI_HDA_DMA_STREAM_TAG",
            "HdaVerb16",
            "Bdl[0].Address",
            "StreamOffset + 0x08",
            "StreamOffset + 0x0C",
            "StreamOffset + 0x12",
            "StreamOffset + 0x18",
            "StreamOffset + 0x1C",
            "StreamCtlLow |= 0x0002U",
            "OMNI_HDA_DMA_PROGRAM_PASS",
            "OMNI_HDA_DMA_RUN_PASS",
            "OMNI_HDA_DMA_LPIB_PROGRESS_PASS",
        ]
        for token in required:
            assert token in source, token


if __name__ == "__main__":
    unittest.main()
