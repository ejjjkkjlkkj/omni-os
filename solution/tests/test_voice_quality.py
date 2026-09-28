import unittest

from omni.voice_quality import inspect_pcm16le


def pcm16(samples: list[int]) -> bytes:
    return b"".join(value.to_bytes(2, "little", signed=True) for value in samples)


class VoiceQualityTests(unittest.TestCase):
    def test_balanced_non_clipping_pcm_is_clean(self):
        report = inspect_pcm16le(pcm16([-1000, 1000] * 256))
        self.assertTrue(report.clean)
        self.assertEqual(report.dc_offset, 0)
        self.assertEqual(report.clipped_samples, 0)

    def test_clipping_is_rejected(self):
        report = inspect_pcm16le(pcm16([32767, -1000, 1000, -1000] * 64))
        self.assertFalse(report.clean)
        self.assertIn("clipping", report.violations)

    def test_large_dc_offset_is_rejected(self):
        report = inspect_pcm16le(pcm16([2000, 2100, 1900, 2000] * 64))
        self.assertFalse(report.clean)
        self.assertIn("dc-offset-channel-0", report.violations)

    def test_opposite_stereo_dc_cannot_cancel_out(self):
        frames = [sample for _ in range(128) for sample in (2000, -2000)]
        report = inspect_pcm16le(pcm16(frames), channels=2)
        self.assertFalse(report.clean)
        self.assertEqual(report.channel_dc_offsets, (2000, -2000))
        self.assertIn("dc-offset-channel-0", report.violations)
        self.assertIn("dc-offset-channel-1", report.violations)

    def test_silence_is_rejected(self):
        report = inspect_pcm16le(pcm16([0] * 256))
        self.assertFalse(report.clean)
        self.assertIn("silence-or-near-silence-channel-0", report.violations)

    def test_frame_alignment_is_mandatory(self):
        with self.assertRaises(ValueError):
            inspect_pcm16le(b"\x00\x00\x00", channels=1)


if __name__ == "__main__":
    unittest.main()
