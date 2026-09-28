from __future__ import annotations

from collections import deque

from .voice_quality import PCM_ALLOWED_CHANNELS, PCM_SAMPLE_RATE

PCM_BYTES_PER_SAMPLE = 2
PCM_DEFAULT_CHUNK_FRAMES = 960
PCM_DEFAULT_MAX_BUFFERED_FRAMES = 9600


class StalePcmGenerationError(RuntimeError):
    """Raised when audio from an interrupted utterance reaches the playback queue."""


class BoundedPcmQueue:
    """Bounded interruptible PCM16LE queue for the VoiceCore renderer path."""

    def __init__(
        self,
        *,
        channels: int = 1,
        sample_rate: int = PCM_SAMPLE_RATE,
        max_buffered_frames: int = PCM_DEFAULT_MAX_BUFFERED_FRAMES,
    ) -> None:
        if sample_rate != PCM_SAMPLE_RATE:
            raise ValueError("VoiceCore PCM queue requires 48000 Hz")
        if channels not in PCM_ALLOWED_CHANNELS:
            raise ValueError("VoiceCore PCM queue supports mono or stereo")
        if max_buffered_frames <= 0:
            raise ValueError("max_buffered_frames must be positive")

        self.channels = channels
        self.sample_rate = sample_rate
        self.max_buffered_frames = max_buffered_frames
        self._frame_bytes = channels * PCM_BYTES_PER_SAMPLE
        self._chunks: deque[bytes] = deque()
        self._head_offset = 0
        self._buffered_frames = 0
        self._generation = 0

    @property
    def buffered_frames(self) -> int:
        return self._buffered_frames

    @property
    def generation(self) -> int:
        return self._generation

    @property
    def empty(self) -> bool:
        return self._buffered_frames == 0

    def capture_generation(self) -> int:
        """Return the token a renderer must present when enqueueing this utterance."""
        return self._generation

    def enqueue(
        self,
        pcm: bytes | bytearray | memoryview,
        *,
        generation: int,
    ) -> None:
        if generation != self._generation:
            raise StalePcmGenerationError(
                f"stale PCM generation {generation}; current generation is {self._generation}"
            )

        data = bytes(pcm)
        if not data:
            return
        if len(data) % self._frame_bytes:
            raise ValueError("PCM chunk is not frame-aligned")

        frames = len(data) // self._frame_bytes
        if frames > self.max_buffered_frames - self._buffered_frames:
            raise BufferError("VoiceCore PCM queue capacity exceeded")

        self._chunks.append(data)
        self._buffered_frames += frames

    def pop_frames(self, max_frames: int = PCM_DEFAULT_CHUNK_FRAMES) -> bytes:
        if max_frames <= 0:
            raise ValueError("max_frames must be positive")
        if self.empty:
            return b""

        wanted = min(max_frames, self._buffered_frames)
        wanted_bytes = wanted * self._frame_bytes
        output = bytearray()

        while wanted_bytes and self._chunks:
            head = self._chunks[0]
            available = len(head) - self._head_offset
            take = min(wanted_bytes, available)
            output.extend(head[self._head_offset : self._head_offset + take])
            self._head_offset += take
            wanted_bytes -= take

            if self._head_offset == len(head):
                self._chunks.popleft()
                self._head_offset = 0

        frames = len(output) // self._frame_bytes
        self._buffered_frames -= frames
        return bytes(output)

    def cancel(self) -> int:
        self._chunks.clear()
        self._head_offset = 0
        self._buffered_frames = 0
        self._generation += 1
        return self._generation
