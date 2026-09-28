import unittest

from omni.voice_release import (
    VOICE_ALLOWED_EVIDENCE_KINDS,
    VOICE_EVIDENCE_SCHEMA,
    VOICE_REQUIRED_GATES,
    evaluate_voice_release,
)


HEAD = "a" * 40


def complete_manifest() -> dict[str, object]:
    gates: dict[str, object] = {}
    for name in VOICE_REQUIRED_GATES:
        gates[name] = {
            "status": "PASS",
            "kind": sorted(VOICE_ALLOWED_EVIDENCE_KINDS[name])[0],
            "evidence_ref": f"test://voice-evidence/{name}",
        }
    return {
        "schema": VOICE_EVIDENCE_SCHEMA,
        "head_sha": HEAD,
        "gates": gates,
    }


class VoiceReleaseTests(unittest.TestCase):
    def test_empty_manifest_cannot_pass(self):
        result = evaluate_voice_release({})
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertEqual(result["missing"], sorted(VOICE_REQUIRED_GATES))
        self.assertIn("schema", result["invalid"])
        self.assertIn("head_sha", result["invalid"])

    def test_legacy_flat_pass_manifest_cannot_pass(self):
        legacy = {name: "PASS" for name in VOICE_REQUIRED_GATES}
        result = evaluate_voice_release(legacy)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertEqual(result["evidence_count"], 0)
        self.assertIn("gates", result["invalid"])

    def test_software_only_is_not_release_ready(self):
        manifest = complete_manifest()
        gates = manifest["gates"]
        assert isinstance(gates, dict)
        for name in (
            "french_intelligibility",
            "french_naturalness",
            "firmware_terms_intelligibility",
            "qemu_ovmf_pcm_path",
            "vmware_uefi_audio_path",
            "physical_hda_audio",
            "physical_speaker_intelligibility",
        ):
            gates.pop(name)

        result = evaluate_voice_release(manifest)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertIn("french_intelligibility", result["missing"])
        self.assertIn("physical_hda_audio", result["missing"])

    def test_not_run_is_a_blocker(self):
        manifest = complete_manifest()
        gates = manifest["gates"]
        assert isinstance(gates, dict)
        gates["french_naturalness"] = {"status": "NOT_RUN"}

        result = evaluate_voice_release(manifest)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertEqual(result["failed"], ["french_naturalness"])

    def test_human_gate_cannot_be_faked_by_ci(self):
        manifest = complete_manifest()
        gates = manifest["gates"]
        assert isinstance(gates, dict)
        gates["french_intelligibility"] = {
            "status": "PASS",
            "kind": "ci",
            "evidence_ref": "github-actions://run/123",
        }

        result = evaluate_voice_release(manifest)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertIn("french_intelligibility:kind", result["invalid"])

    def test_physical_speaker_gate_requires_physical_human_listening(self):
        manifest = complete_manifest()
        gates = manifest["gates"]
        assert isinstance(gates, dict)
        gates["physical_speaker_intelligibility"] = {
            "status": "PASS",
            "kind": "human-listening",
            "evidence_ref": "lab://listener/session-1",
        }

        result = evaluate_voice_release(manifest)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertIn("physical_speaker_intelligibility:kind", result["invalid"])

    def test_pass_requires_nonempty_evidence_reference(self):
        manifest = complete_manifest()
        gates = manifest["gates"]
        assert isinstance(gates, dict)
        record = gates["renderer_determinism"]
        assert isinstance(record, dict)
        record["evidence_ref"] = "   "

        result = evaluate_voice_release(manifest)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertIn("renderer_determinism:evidence_ref", result["invalid"])

    def test_expected_head_mismatch_is_a_blocker(self):
        manifest = complete_manifest()
        result = evaluate_voice_release(manifest, expected_head_sha="b" * 40)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertIn("head_sha-mismatch", result["invalid"])

    def test_unexpected_gate_is_a_blocker(self):
        manifest = complete_manifest()
        gates = manifest["gates"]
        assert isinstance(gates, dict)
        gates["robotic_voice_override"] = {
            "status": "PASS",
            "kind": "ci",
            "evidence_ref": "test://invalid-gate",
        }

        result = evaluate_voice_release(manifest)
        self.assertEqual(result["status"], "VOICE_RELEASE_INCOMPLETE")
        self.assertIn("gate:robotic_voice_override", result["unexpected"])

    def test_exact_complete_manifest_passes(self):
        manifest = complete_manifest()
        result = evaluate_voice_release(manifest, expected_head_sha=HEAD)
        self.assertEqual(result["status"], "VOICE_RELEASE_PASS")
        self.assertEqual(result["blockers"], [])
        self.assertEqual(result["evidence_count"], len(VOICE_REQUIRED_GATES))


if __name__ == "__main__":
    unittest.main()
