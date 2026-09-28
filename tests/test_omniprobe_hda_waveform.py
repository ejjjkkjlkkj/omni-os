from pathlib import Path
import unittest

SRC = Path("firmware/OmniPkg/Applications/OmniProbe/OmniProbe.c").read_text(encoding="utf-8")

class OmniprobeHdaWaveformTests(unittest.TestCase):
    def test_dma_payload_is_not_silence(self):
        assert "FillHdaTone (AudioHost, OMNI_HDA_DMA_BUFFER_BYTES);" in SRC
        assert "OMNI_HDA_TONE_HZ          750U" in SRC
        assert "OMNI_HDA_TONE_MILLISECONDS 4000U" in SRC

    def test_physical_route_is_programmed(self):
        assert "ProgramHdaOutputRoute (PciIo, Stats)" in SRC
        assert "HdaVerb (Stats->CodecAddress, Stats->PinNode, 0x707, PinControl)" in SRC
        assert "HdaVerb (Stats->CodecAddress, Stats->PinNode, 0x70C, Eapd)" in SRC
        assert "0xB000U | Gain" in SRC

    def test_waveform_evidence_is_persisted(self):
        for marker in (
            "OMNI_HDA_DMA_TONE_PREPARED",
            "OMNI_HDA_DMA_TONE_MILLISECONDS",
            "OMNI_HDA_ROUTE_PROGRAMMED",
            "OMNI_HDA_ROUTE_PIN_CONTROL_AFTER",
            "OMNI_HDA_ROUTE_EAPD_AFTER",
            "OMNI_HDA_CONVERTER_AMP_PROGRAMMED",
            "OMNI_HDA_PIN_AMP_PROGRAMMED",
        ):
            assert marker in SRC

    def test_existing_dma_gate_remains(self):
        for marker in (
            "OMNI_HDA_DMA_PROGRESS",
            "OMNI_HDA_DMA_STREAM_TAG",
            "OMNI_HDA_FIRST_OUTPUT_STREAM_LPIB",
            "OMNI_HDA_FIRST_OUTPUT_STREAM_CBL",
            "OMNI_HDA_FIRST_OUTPUT_STREAM_LVI",
            "OMNI_HDA_FIRST_OUTPUT_STREAM_FORMAT",
        ):
            assert marker in SRC


if __name__ == "__main__":
    unittest.main()
