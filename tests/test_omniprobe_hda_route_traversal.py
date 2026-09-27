from __future__ import annotations

import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "firmware/OmniPkg/Applications/OmniProbe/OmniProbe.c"


def test_hda_route_resolver_walks_pin_mixer_dac_path() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    required = (
        "HdaGetConnectionNodeAt",
        "ResolveHdaOutputConverter",
        "HdaVerb (Stats->CodecAddress, CurrentNode, 0xF01, 0)",
        "HdaVerb (Codec, Node, 0xF02, BaseIndex)",
        "0x14 -> 0x0c -> 0x02",
        "Stats->RouteResolved = 1",
        "Stats->RouteIntermediateNode = CurrentNode",
        "Stats->RouteResolvedDepth",
    )
    for token in required:
        assert token in source, token


def test_connection_ranges_fail_closed_instead_of_guessing() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    helper = source[source.index("STATIC EFI_STATUS HdaGetConnectionNodeAt") :]
    helper = helper[: helper.index("STATIC EFI_STATUS ResolveHdaOutputConverter")]
    assert "RangeMask" in helper
    assert "return EFI_UNSUPPORTED;" in helper


def test_intermediate_mixer_input_amp_is_unmuted() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    route = source[source.index("STATIC EFI_STATUS ProgramHdaOutputRoute") :]
    route = route[: route.index("STATIC EFI_STATUS ProgramHdaDmaProof")]
    assert "RouteIntermediateWidgetCaps & (1U << 1)" in route
    assert "Stats->RouteIntermediateNode" in route
    assert "Stats->RouteIntermediateConnectionIndex" in route
    assert "0x7000U" in route
    assert "Stats->RouteIntermediateAmpProgrammed = 1" in route


def test_route_resolution_evidence_is_persisted() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    for marker in (
        "OMNI_HDA_ROUTE_RESOLVED",
        "OMNI_HDA_ROUTE_RESOLVED_DEPTH",
        "OMNI_HDA_ROUTE_INTERMEDIATE_NODE",
        "OMNI_HDA_ROUTE_INTERMEDIATE_CONNECTION_INDEX",
        "OMNI_HDA_ROUTE_INTERMEDIATE_AMP_PROGRAMMED",
    ):
        assert marker in source, marker


def test_selector_routes_follow_only_the_active_connection() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    resolver = source[source.index("STATIC EFI_STATUS ResolveHdaOutputConverter") :]
    resolver = resolver[: resolver.index("STATIC VOID ProbeHdaCodecTopology")]
    assert "false route evidence" in resolver
    assert "return EFI_COMPROMISED_DATA;" in resolver
    assert "ScanCount = 1;" in resolver
    assert "Index = (ScanCount == 1) ? PreferredIndex : TryIndex;" in resolver


def test_amp_caps_fall_back_to_audio_function_group_without_override() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    helper = source[source.index("STATIC EFI_STATUS HdaGetAmplifierCaps") :]
    helper = helper[: helper.index("STATIC EFI_STATUS ProgramHdaOutputRoute")]
    assert "(WidgetCaps & (1U << 3)) != 0" in helper
    assert "Stats->AudioFunctionGroup" in helper
    assert "(Parameter != 0x0DU) && (Parameter != 0x12U)" in helper

    route = source[source.index("STATIC EFI_STATUS ProgramHdaOutputRoute") :]
    route = route[: route.index("STATIC EFI_STATUS ProgramHdaDmaProof")]
    assert route.count("HdaGetAmplifierCaps (") == 3
    assert "Stats->RouteIntermediateWidgetCaps" in route
    assert "Stats->ConverterWidgetCaps" in route
    assert "Stats->PinWidgetCaps" in route
