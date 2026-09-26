from __future__ import annotations

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "firmware/OmniPkg/Applications/OmniProbe/OmniProbe.c"


def test_gpu_hdmi_controllers_are_skipped_on_first_pass() -> None:
    """The physical ASUS M1603QA run selected 1002:1637 (GPU HDMI audio), whose
    codec 0x1002AA01 has no analog pin, so no speaker route could exist. The
    probe must prefer a non-GPU HDA controller and record why."""
    source = SOURCE.read_text(encoding="utf-8")
    body = source[source.index("for (Pass = 0; (Pass < 2) && (Stats->MmioValid == 0); ++Pass)"):]
    body = body[: body.index("FreePool (Handles)")]
    assert re.search(r"\(Pass == 0\) && \(\(VendorId == 0x1002U\) \|\| \(VendorId == 0x10DEU\)\)", body)
    assert "Stats->GpuHdmiControllersSkipped++" in body
    # Counters must not double on the fallback pass.
    assert "if (Pass == 0) Stats->Controllers++;" in body
    assert "if (Pass == 0) Stats->PciHandles++;" in body
    # The skip happens before the controller is claimed (MMIO/reset/DMA).
    assert body.index("GpuHdmiControllersSkipped++") < body.index("ProgramHdaDmaProof (PciIo, SystemTable, Stats);")


def test_selection_evidence_is_persisted_and_printed() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    for key in ("OMNI_HDA_GPU_HDMI_SKIPPED", "OMNI_HDA_SELECTION_PASS"):
        assert f'FileWriteStat (File, "{key}"' in source, key
        assert f'WriteStat ("{key}"' in source, key
