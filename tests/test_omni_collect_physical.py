from __future__ import annotations

from pathlib import Path

COLLECTOR = Path("tools/omni_collect_physical.ps1").read_text(encoding="utf-8")


def test_physical_collector_requires_explicit_keyboard_and_navigation_proof() -> None:
    assert "OMNI_KEYBOARD_PASS" in COLLECTOR
    assert "OMNI_NAVIGATION_INPUT_PASS" in COLLECTOR
    assert "$gates.KEYBOARD -ne 'PASS'" in COLLECTOR
    assert "$gates.NAVIGATION_INPUT -ne 'PASS'" in COLLECTOR


def test_physical_collector_requires_resolved_programmed_audio_route() -> None:
    for token in (
        "OMNI_HDA_ROUTE_RESOLVED",
        "OMNI_HDA_DMA_PROGRESS",
        "OMNI_HDA_ROUTE_PROGRAMMED",
    ):
        assert token in COLLECTOR


def test_physical_collector_keeps_human_audibility_as_explicit_gate() -> None:
    assert "[switch]$AudibleSpeakerConfirmed" in COLLECTOR
    assert "internal-speaker tone requires human confirmation" in COLLECTOR
    assert "$verdict.releaseReady = ($blockers.Count -eq 0)" in COLLECTOR


def test_physical_collector_requires_screen_reader_physical_proof_when_bound() -> None:
    assert "QEVARYNOX-PHYSICAL-PROOF.TXT is missing" in COLLECTOR
    assert "screen-reader physical proof has no STATUS field" in COLLECTOR
    assert "FAIL|ERROR|PENDING|UNPROVEN" in COLLECTOR
