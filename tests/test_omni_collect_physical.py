from __future__ import annotations

from pathlib import Path
import unittest

COLLECTOR = Path("tools/omni_collect_physical.ps1").read_text(encoding="utf-8")


class OmniCollectPhysicalTests(unittest.TestCase):
    def test_physical_collector_requires_explicit_keyboard_and_navigation_proof(self) -> None:
        assert "OMNI_KEYBOARD_PASS" in COLLECTOR
        assert "OMNI_NAVIGATION_INPUT_PASS" in COLLECTOR
        assert "$gates.KEYBOARD -ne 'PASS'" in COLLECTOR
        assert "$gates.NAVIGATION_INPUT -ne 'PASS'" in COLLECTOR


    def test_physical_collector_requires_resolved_programmed_audio_route(self) -> None:
        for token in (
            "OMNI_HDA_ROUTE_RESOLVED",
            "OMNI_HDA_DMA_PROGRESS",
            "OMNI_HDA_ROUTE_PROGRAMMED",
        ):
            assert token in COLLECTOR


    def test_physical_collector_keeps_human_audibility_as_explicit_gate(self) -> None:
        assert "[switch]$AudibleSpeakerConfirmed" in COLLECTOR
        assert "internal-speaker tone requires human confirmation" in COLLECTOR
        assert "$verdict.releaseReady = ($blockers.Count -eq 0)" in COLLECTOR


    def test_physical_collector_requires_screen_reader_physical_proof_when_bound(self) -> None:
        assert "QEVARYNOX-PHYSICAL-PROOF.TXT is missing" in COLLECTOR
        assert "STATUS = 'PASS'" in COLLECTOR
        assert "screen-reader proof $k is MISSING" in COLLECTOR
        assert "expected '$expectedValue'" in COLLECTOR


    def test_physical_collector_requires_complete_screen_reader_runtime_proof(self) -> None:
        required = (
            "STATUS = 'PASS'",
            "HII_GRAPH_SPEECH_MODE = 'CLEAR_LETTERNAME_SPELLING_FR_V3'",
            "HDA_CONTROLLER_SELECTION = 'PREFERRED_AMD_1022_15E3'",
            "HDA_CODEC_VENDOR_DEVICE = '0x10ec0256'",
            "HDA_CODEC_SELECTION = 'REALTEK_10EC_0256'",
            "HDA_GRAPH_SEARCH_LIVE = 'PASS'",
            "HDA_SELECTOR_APPLY_LIVE = 'PASS'",
            "HDA_ROUTE_POWER_D0 = 'PASS'",
            "HDA_ROUTE_AMPLIFIERS = 'PASS'",
            "HDA_EAPD_POLICY = 'PASS'",
            "HDA_DAC_STREAM_READBACK = 'PASS'",
            "HDA_PIN_CONTROL_READBACK = 'PASS'",
            "HDA_OUTPUT_PATH_CONFIGURATION = 'PASS'",
            "HII_GRAPH_SPEECH_DMA = 'PASS'",
            "LPIB_PROGRESS = 'PASS'",
            "PHYSICAL_ASUS_M1603QA_HDA_RUNTIME = 'PASS'",
            "PHYSICAL_ASUS_M1603QA_CODEC = 'REALTEK_10EC_0256'",
            "PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN = 'PASS'",
            "HII_GRAPH_NAV_REQUIRED_EVENTS = 'PASS'",
            "HII_GRAPH_NAV_REALTIME = 'PASS'",
            "HII_GRAPH_SPEECH_DMA_REUSE = 'PASS'",
        )
        for token in required:
            assert token in COLLECTOR, token
        assert "^([A-Z0-9_]+)=(.*)$" in COLLECTOR
        assert "screen-reader proof $k is MISSING" in COLLECTOR
        assert "OrdinalIgnoreCase" in COLLECTOR


    def test_physical_collector_verifies_bound_screen_reader_hash(self) -> None:
        assert 'EFI\\OMNI\\SCREENREADER.EFI' in COLLECTOR
        assert 'Get-FileHash $readerPath -Algorithm SHA256' in COLLECTOR
        assert 'bound screen-reader binary is missing from collected media' in COLLECTOR
        assert 'screen-reader SHA-256 mismatch' in COLLECTOR
        assert 'binding.screenReaderSha256' in COLLECTOR


if __name__ == "__main__":
    unittest.main()
