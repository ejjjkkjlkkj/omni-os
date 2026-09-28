from __future__ import annotations

import re
from collections.abc import Mapping

VOICE_EVIDENCE_SCHEMA = "omniexec.voice-release-evidence.v1"

VOICE_SOFTWARE_GATES = frozenset(
    {
        "semantic_frontend",
        "phoneme_stream_abi",
        "renderer_48khz_pcm",
        "renderer_determinism",
        "pcm_cleanliness",
        "bounded_text_and_stream",
        "bounded_renderer_memory",
        "first_audio_latency",
        "interrupt_latency",
        "long_run_stability",
        "reproducible_voice_artifact",
    }
)

VOICE_PERCEPTUAL_GATES = frozenset(
    {
        "french_intelligibility",
        "french_naturalness",
        "firmware_terms_intelligibility",
    }
)

VOICE_HARDWARE_GATES = frozenset(
    {
        "qemu_ovmf_pcm_path",
        "vmware_uefi_audio_path",
        "physical_hda_audio",
        "physical_speaker_intelligibility",
    }
)

VOICE_REQUIRED_GATES = (
    VOICE_SOFTWARE_GATES | VOICE_PERCEPTUAL_GATES | VOICE_HARDWARE_GATES
)

VOICE_ALLOWED_STATUSES = frozenset(
    {
        "PASS",
        "FAIL",
        "NOT_RUN",
        "MISSING",
        "BLOCKED",
        "SKIP",
        "CANCELLED",
        "TIMED_OUT",
    }
)

VOICE_ALLOWED_EVIDENCE_KINDS = {
    "semantic_frontend": frozenset({"ci"}),
    "phoneme_stream_abi": frozenset({"ci"}),
    "renderer_48khz_pcm": frozenset({"ci", "measurement"}),
    "renderer_determinism": frozenset({"ci"}),
    "pcm_cleanliness": frozenset({"ci", "measurement"}),
    "bounded_text_and_stream": frozenset({"ci"}),
    "bounded_renderer_memory": frozenset({"measurement"}),
    "first_audio_latency": frozenset({"measurement"}),
    "interrupt_latency": frozenset({"measurement"}),
    "long_run_stability": frozenset({"ci", "measurement"}),
    "reproducible_voice_artifact": frozenset({"ci"}),
    "french_intelligibility": frozenset({"human-listening"}),
    "french_naturalness": frozenset({"human-listening"}),
    "firmware_terms_intelligibility": frozenset({"human-listening"}),
    "qemu_ovmf_pcm_path": frozenset({"ci"}),
    "vmware_uefi_audio_path": frozenset({"ci"}),
    "physical_hda_audio": frozenset({"hardware-measurement"}),
    "physical_speaker_intelligibility": frozenset({"human-physical-listening"}),
}

VOICE_MANIFEST_KEYS = frozenset({"schema", "head_sha", "gates"})
_SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def _valid_head_sha(value: object) -> bool:
    return isinstance(value, str) and _SHA_RE.fullmatch(value) is not None


def evaluate_voice_release(
    manifest: Mapping[str, object],
    *,
    expected_head_sha: str | None = None,
) -> dict[str, object]:
    """Evaluate fail-closed VoiceCore release evidence.

    PASS is possible only when every required gate has explicit evidence of an
    allowed kind and the manifest is bound to a concrete Git commit.
    """
    invalid: list[str] = []
    unexpected: list[str] = []

    schema = manifest.get("schema")
    if schema != VOICE_EVIDENCE_SCHEMA:
        invalid.append("schema")

    head_sha = manifest.get("head_sha")
    if not _valid_head_sha(head_sha):
        invalid.append("head_sha")
    if expected_head_sha is not None:
        if not _valid_head_sha(expected_head_sha):
            raise ValueError("expected_head_sha must be a lowercase 40-character Git SHA")
        if head_sha != expected_head_sha:
            invalid.append("head_sha-mismatch")

    if any(not isinstance(name, str) or not name for name in manifest.keys()):
        invalid.append("manifest-key")
    unexpected.extend(
        f"manifest:{name}"
        for name in sorted(
            name
            for name in manifest.keys()
            if isinstance(name, str) and name not in VOICE_MANIFEST_KEYS
        )
    )

    raw_gates = manifest.get("gates")
    if not isinstance(raw_gates, Mapping):
        invalid.append("gates")
        gates: Mapping[object, object] = {}
    else:
        gates = raw_gates

    supplied_names = {name for name in gates.keys() if isinstance(name, str)}
    missing = sorted(VOICE_REQUIRED_GATES - supplied_names)
    unexpected.extend(
        f"gate:{name}" for name in sorted(supplied_names - VOICE_REQUIRED_GATES)
    )
    if any(not isinstance(name, str) or not name for name in gates.keys()):
        invalid.append("gate-name")

    failed: list[str] = []
    valid_evidence_count = 0

    for name in sorted(VOICE_REQUIRED_GATES & supplied_names):
        record = gates[name]
        if not isinstance(record, Mapping):
            invalid.append(f"{name}:record")
            continue

        status = record.get("status")
        if status not in VOICE_ALLOWED_STATUSES:
            invalid.append(f"{name}:status")
            continue

        if status != "PASS":
            failed.append(name)
            continue

        kind = record.get("kind")
        evidence_ref = record.get("evidence_ref")
        allowed_kinds = VOICE_ALLOWED_EVIDENCE_KINDS[name]

        if kind not in allowed_kinds:
            invalid.append(f"{name}:kind")
        if (
            not isinstance(evidence_ref, str)
            or not evidence_ref.strip()
            or len(evidence_ref) > 1024
        ):
            invalid.append(f"{name}:evidence_ref")

        extra_fields = sorted(
            key
            for key in record.keys()
            if isinstance(key, str)
            and key not in {"status", "kind", "evidence_ref"}
        )
        unexpected.extend(f"{name}:{key}" for key in extra_fields)

        if (
            kind in allowed_kinds
            and isinstance(evidence_ref, str)
            and bool(evidence_ref.strip())
            and len(evidence_ref) <= 1024
        ):
            valid_evidence_count += 1

    blockers = sorted(set(missing) | set(failed) | set(invalid) | set(unexpected))

    return {
        "status": "VOICE_RELEASE_PASS" if not blockers else "VOICE_RELEASE_INCOMPLETE",
        "schema": schema,
        "head_sha": head_sha,
        "blockers": blockers,
        "missing": missing,
        "failed": failed,
        "invalid": sorted(set(invalid)),
        "unexpected": sorted(set(unexpected)),
        "software_required": sorted(VOICE_SOFTWARE_GATES),
        "perceptual_required": sorted(VOICE_PERCEPTUAL_GATES),
        "hardware_required": sorted(VOICE_HARDWARE_GATES),
        "evidence_count": valid_evidence_count,
    }
