"""In-memory Windows audio output for the speech pipeline (no temporary WAV).

``WaveOutSink`` is a ``SpeechController`` sink: it receives float32 48 kHz mono
chunks from the renderer, converts them to PCM16 and queues them on the default
waveOut device. ``stop()`` (called on every interruption) resets the device so
queued audio is dropped at once. Backpressure keeps at most ``max_queued_ms`` of
audio in flight, so a cancel never has seconds of stale speech behind it.
"""
from __future__ import annotations

import ctypes
import sys
import threading
import time
from array import array
from ctypes import wintypes

RATE = 48_000
_WHDR_DONE = 0x1


class _WaveFormat(ctypes.Structure):
    _fields_ = [("wFormatTag", wintypes.WORD), ("nChannels", wintypes.WORD), ("nSamplesPerSec", wintypes.DWORD),
                ("nAvgBytesPerSec", wintypes.DWORD), ("nBlockAlign", wintypes.WORD), ("wBitsPerSample", wintypes.WORD),
                ("cbSize", wintypes.WORD)]


class _WaveHdr(ctypes.Structure):
    pass


_WaveHdr._fields_ = [("lpData", ctypes.c_void_p), ("dwBufferLength", wintypes.DWORD), ("dwBytesRecorded", wintypes.DWORD),
                     ("dwUser", ctypes.c_size_t), ("dwFlags", wintypes.DWORD), ("dwLoops", wintypes.DWORD),
                     ("lpNext", ctypes.POINTER(_WaveHdr)), ("reserved", ctypes.c_size_t)]


def float_to_pcm16(chunk) -> bytes:
    out = array("h", (max(-32767, min(32767, int(round(x * 32767.0)))) for x in chunk))
    if sys.byteorder != "little":
        out.byteswap()
    return out.tobytes()


class WaveOutSink:
    """Callable sink: ``sink(chunk)`` plays, ``stop()`` silences, ``close()`` releases."""

    def __init__(self, *, device: int = -1, max_queued_ms: int = 250, volume: float = 1.0):
        if sys.platform != "win32":
            raise OSError("WaveOutSink requires Windows")
        self._w = ctypes.WinDLL("winmm")
        self._w.waveOutOpen.argtypes = [ctypes.POINTER(ctypes.c_void_p), wintypes.UINT, ctypes.POINTER(_WaveFormat),
                                        ctypes.c_size_t, ctypes.c_size_t, wintypes.DWORD]
        for name in ("waveOutPrepareHeader", "waveOutUnprepareHeader", "waveOutWrite"):
            getattr(self._w, name).argtypes = [ctypes.c_void_p, ctypes.POINTER(_WaveHdr), wintypes.UINT]
        for name in ("waveOutReset", "waveOutClose"):
            getattr(self._w, name).argtypes = [ctypes.c_void_p]
        fmt = _WaveFormat(1, 1, RATE, RATE * 2, 2, 16, 0)
        self._handle = ctypes.c_void_p()
        code = self._w.waveOutOpen(ctypes.byref(self._handle), device & 0xFFFFFFFF, ctypes.byref(fmt), 0, 0, 0)
        if code:
            raise OSError(f"waveOutOpen failed ({code})")
        self._lock = threading.Lock()
        self._pending: list[tuple[_WaveHdr, ctypes.Array]] = []
        self._max_bytes = RATE * 2 * max_queued_ms // 1000
        self.volume = volume
        self.played_bytes = 0
        self._epoch = 0  # bumped by stop(): a chunk waiting for room must not play after it
        self.first_write: float | None = None

    def _reap(self) -> int:
        keep, queued = [], 0
        for hdr, buf in self._pending:
            if hdr.dwFlags & _WHDR_DONE:
                self._w.waveOutUnprepareHeader(self._handle, ctypes.byref(hdr), ctypes.sizeof(hdr))
            else:
                keep.append((hdr, buf))
                queued += hdr.dwBufferLength
        self._pending = keep
        return queued

    def __call__(self, chunk) -> bool:
        data = float_to_pcm16(x * self.volume for x in chunk) if self.volume != 1.0 else float_to_pcm16(chunk)
        epoch = self._epoch
        while True:
            with self._lock:
                if not self._handle or epoch != self._epoch:
                    return False
                if self._reap() < self._max_bytes:
                    buf = ctypes.create_string_buffer(data, len(data))
                    hdr = _WaveHdr(ctypes.cast(buf, ctypes.c_void_p), len(data), 0, 0, 0, 0, None, 0)
                    self._w.waveOutPrepareHeader(self._handle, ctypes.byref(hdr), ctypes.sizeof(hdr))
                    self._w.waveOutWrite(self._handle, ctypes.byref(hdr), ctypes.sizeof(hdr))
                    self._pending.append((hdr, buf))
                    self.played_bytes += len(data)
                    if self.first_write is None:
                        self.first_write = time.perf_counter()
                    return True
            time.sleep(0.005)  # device is 250 ms ahead: backpressure to the renderer

    def stop(self) -> None:
        with self._lock:
            self._epoch += 1
            if self._handle:
                self._w.waveOutReset(self._handle)  # marks every queued header DONE immediately
                self._reap()

    def queued_ms(self) -> float:
        with self._lock:
            return self._reap() / (RATE * 2) * 1000

    def drain(self, timeout: float = 30.0) -> bool:
        end = time.monotonic() + timeout
        while self.queued_ms() > 0:
            if time.monotonic() > end:
                return False
            time.sleep(0.01)
        return True

    def close(self) -> None:
        self.stop()
        with self._lock:
            if self._handle:
                self._w.waveOutClose(self._handle)
                self._handle = ctypes.c_void_p()

    def __enter__(self) -> "WaveOutSink":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()
