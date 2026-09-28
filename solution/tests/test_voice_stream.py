import unittest

from omni.voice_stream import (
    PCM_DEFAULT_CHUNK_FRAMES,
    PCM_DEFAULT_MAX_BUFFERED_FRAMES,
    BoundedPcmQueue,
    StalePcmGenerationError,
)


def pcm_frames(frames: int, channels: int = 1, sample: int = 1000) -> bytes:
    encoded = sample.to_bytes(2, "little", signed=True)
    return encoded * frames * channels


class VoiceStreamTests(unittest.TestCase):
    def test_queue_is_bounded(self):
        queue = BoundedPcmQueue(max_buffered_frames=8)
        generation = queue.capture_generation()
        queue.enqueue(pcm_frames(8), generation=generation)
        with self.assertRaises(BufferError):
            queue.enqueue(pcm_frames(1), generation=generation)
        self.assertEqual(queue.buffered_frames, 8)

    def test_cancel_is_immediate_and_advances_generation(self):
        queue = BoundedPcmQueue()
        generation = queue.capture_generation()
        queue.enqueue(pcm_frames(4000), generation=generation)
        self.assertGreater(queue.buffered_frames, 0)
        new_generation = queue.cancel()
        self.assertEqual(new_generation, generation + 1)
        self.assertTrue(queue.empty)
        self.assertEqual(queue.pop_frames(), b"")

    def test_cancel_rejects_late_renderer_output(self):
        queue = BoundedPcmQueue()
        stale_generation = queue.capture_generation()
        queue.cancel()
        with self.assertRaises(StalePcmGenerationError):
            queue.enqueue(pcm_frames(10), generation=stale_generation)
        self.assertTrue(queue.empty)

    def test_new_generation_can_enqueue_after_cancel(self):
        queue = BoundedPcmQueue()
        queue.cancel()
        generation = queue.capture_generation()
        queue.enqueue(pcm_frames(10), generation=generation)
        self.assertEqual(queue.buffered_frames, 10)

    def test_pop_never_exceeds_requested_frames(self):
        queue = BoundedPcmQueue()
        generation = queue.capture_generation()
        queue.enqueue(
            pcm_frames(PCM_DEFAULT_CHUNK_FRAMES * 2),
            generation=generation,
        )
        chunk = queue.pop_frames()
        self.assertEqual(len(chunk), PCM_DEFAULT_CHUNK_FRAMES * 2)
        self.assertEqual(queue.buffered_frames, PCM_DEFAULT_CHUNK_FRAMES)

    def test_partial_chunk_preserves_pcm_order(self):
        queue = BoundedPcmQueue(max_buffered_frames=4)
        samples = [100, 200, 300, 400]
        data = b"".join(value.to_bytes(2, "little", signed=True) for value in samples)
        queue.enqueue(data, generation=queue.capture_generation())
        self.assertEqual(queue.pop_frames(1), data[:2])
        self.assertEqual(queue.pop_frames(3), data[2:])
        self.assertTrue(queue.empty)

    def test_stereo_alignment_is_enforced(self):
        queue = BoundedPcmQueue(channels=2)
        generation = queue.capture_generation()
        with self.assertRaises(ValueError):
            queue.enqueue(b"\x00\x00", generation=generation)
        queue.enqueue(pcm_frames(1, channels=2), generation=generation)
        self.assertEqual(queue.buffered_frames, 1)

    def test_default_buffer_is_small_and_finite(self):
        self.assertEqual(PCM_DEFAULT_MAX_BUFFERED_FRAMES, 9600)
        self.assertEqual(PCM_DEFAULT_CHUNK_FRAMES, 960)


if __name__ == "__main__":
    unittest.main()
