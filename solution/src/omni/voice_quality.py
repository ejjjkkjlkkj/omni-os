from __future__ import annotations

from dataclasses import dataclass
from math import isqrt

PCM_SAMPLE_RATE = 48_000
PCM_ALLOWED_CHANNELS = (1, 2)
PCM_MIN_RMS = 48
PCM_MAX_ABS_DC = 512


def _mean_toward_zero(values: list[int]) -> int:
    total = sum(values)
    if total >= 0:
        return total // len(values)
    return -((-total) // len(values))


@dataclass(frozen=True, slots=True)
class PcmQualityReport:
    sample_rate: int
    channels: int
    frames: int
    peak: int
    rms: int
    dc_offset: int
    channel_rms: tuple[int, ...]
    channel_dc_offsets: tuple[int, ...]
    clipped_samples: int
    violations: tuple[str, ...]

    @property
    def clean(self) -> bool:
        return not self.violations

    def to_dict(self) -> dict[str, object]:
        return {
            "sample_rate": self.sample_rate,
            "channels": self.channels,
            "frames": self.frames,
            "peak": self.peak,
            "rms": self.rms,
            "dc_offset": self.dc_offset,
            "channel_rms": list(self.channel_rms),
            "channel_dc_offsets": list(self.channel_dc_offsets),
            "clipped_samples": self.clipped_samples,
            "violations": list(self.violations),
            "status": "VOICE_PCM_CLEAN" if self.clean else "VOICE_PCM_REJECTED",
        }


def inspect_pcm16le(
    pcm: bytes | bytearray | memoryview,
    *,
    channels: int = 1,
    sample_rate: int = PCM_SAMPLE_RATE,
) -> PcmQualityReport:
    """Validate the hard cleanliness contract for VoiceCore PCM output.

    This gate checks transport/audio hygiene only. It does not claim naturalness
    or intelligibility, which require acoustic and human-listening validation.
    """
    if sample_rate != PCM_SAMPLE_RATE:
        raise ValueError("VoiceCore PCM must be 48000 Hz")
    if channels not in PCM_ALLOWED_CHANNELS:
        raise ValueError("VoiceCore PCM channels must be mono or stereo")

    data = bytes(pcm)
    frame_bytes = channels * 2
    if not data:
        raise ValueError("PCM buffer is empty")
    if len(data) % frame_bytes:
        raise ValueError("PCM buffer is not frame-aligned")

    samples = [
        int.from_bytes(data[i : i + 2], "little", signed=True)
        for i in range(0, len(data), 2)
    ]
    frames = len(samples) // channels
    peak = max(abs(value) for value in samples)
    clipped = sum(value in (-32768, 32767) for value in samples)

    channel_values = tuple(samples[index::channels] for index in range(channels))
    channel_dc = tuple(_mean_toward_zero(values) for values in channel_values)
    channel_rms = tuple(
        isqrt(sum(value * value for value in values) // len(values))
        for values in channel_values
    )

    dc_offset = max(channel_dc, key=abs)
    rms = min(channel_rms)

    violations: list[str] = []
    if clipped:
        violations.append("clipping")
    for index, value in enumerate(channel_dc):
        if abs(value) > PCM_MAX_ABS_DC:
            violations.append(f"dc-offset-channel-{index}")
    for index, value in enumerate(channel_rms):
        if value < PCM_MIN_RMS:
            violations.append(f"silence-or-near-silence-channel-{index}")

    return PcmQualityReport(
        sample_rate=sample_rate,
        channels=channels,
        frames=frames,
        peak=peak,
        rms=rms,
        dc_offset=dc_offset,
        channel_rms=channel_rms,
        channel_dc_offsets=channel_dc,
        clipped_samples=clipped,
        violations=tuple(violations),
    )
