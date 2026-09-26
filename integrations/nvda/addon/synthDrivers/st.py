# ST speech synthesizer driver for NVDA (x64, NVDA 2025.1+ / Python 3.13).
#
# Loads st_synth.dll through the ST C ABI v1 and plays its 48 kHz float chunks
# through nvwave from memory. Voices: the compact ST voices are always there
# (DLL bundled with the add-on); neural voices appear when the add-on can find a
# full ST release (st_home.txt next to this file, or the ST_HOME environment
# variable) containing st_synth.dll and neural\.
#
# Threading: speak() only queues; one background thread renders. cancel() bumps
# a generation counter, calls st_engine_cancel_v1 (thread-safe) and stops the
# player, so stale audio never plays after a newer utterance.

import ctypes
import os
import queue
import threading
from array import array
from collections import OrderedDict

import nvwave
from logHandler import log
from speech.commands import BreakCommand, IndexCommand
from synthDriverHandler import SynthDriver as BaseSynthDriver, VoiceInfo, synthDoneSpeaking, synthIndexReached

HERE = os.path.dirname(os.path.abspath(__file__))
RATE = 48000
COMPACT = OrderedDict([
	("fr:female", ("ST compact, français, femme", "fr_FR")),
	("fr:male", ("ST compact, français, homme", "fr_FR")),
	("en:female", ("ST compact, English, female", "en_US")),
	("en:male", ("ST compact, English, male", "en_US")),
])
NEURAL = OrderedDict([
	("fr:ff_siwis", ("ST neural, français, Siwis", "fr_FR")),
	("en:af_heart", ("ST neural, English, Heart", "en_US")),
	("en:am_michael", ("ST neural, English, Michael", "en_US")),
	("en:bf_emma", ("ST neural, English (UK), Emma", "en_GB")),
])
_CALLBACK = ctypes.CFUNCTYPE(ctypes.c_uint8, ctypes.POINTER(ctypes.c_float), ctypes.c_size_t, ctypes.c_uint32, ctypes.c_void_p)


def _st_home():
	for candidate in (os.environ.get("ST_HOME"), _read(os.path.join(HERE, "st_home.txt")), HERE):
		if candidate and os.path.isfile(os.path.join(candidate, "st_synth.dll")):
			return candidate
	return None


def _read(path):
	try:
		with open(path, encoding="utf-8") as f:
			return f.read().strip()
	except OSError:
		return None


class _Lib:
	def __init__(self, home):
		self.dll = ctypes.CDLL(os.path.join(home, "st_synth.dll"))
		d = self.dll
		d.st_engine_create_v1.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
		d.st_engine_create_v1.restype = ctypes.c_int32
		d.st_engine_stream_v1.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t, _CALLBACK, ctypes.c_void_p]
		d.st_engine_stream_v1.restype = ctypes.c_int32
		d.st_engine_cancel_v1.argtypes = [ctypes.c_void_p]
		d.st_engine_cancel_v1.restype = None
		d.st_engine_set_rate_v1.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
		d.st_engine_set_rate_v1.restype = ctypes.c_int32
		d.st_engine_destroy_v1.argtypes = [ctypes.c_void_p]
		d.st_engine_destroy_v1.restype = None
		d.st_last_error_v1.argtypes = [ctypes.c_char_p, ctypes.c_size_t]
		d.st_last_error_v1.restype = ctypes.c_size_t

	def error(self):
		buf = ctypes.create_string_buffer(512)
		self.dll.st_last_error_v1(buf, len(buf))
		return buf.value.decode("utf-8", "replace")

	def create(self, backend, lang, voice, rate):
		cfg = ('{"backend":"%s","lang":"%s","voice":"%s","rate":%d}' % (backend, lang, voice, rate)).encode()
		h = ctypes.c_void_p()
		code = self.dll.st_engine_create_v1(cfg, len(cfg), ctypes.byref(h))
		if code:
			raise RuntimeError("ST create failed (%d): %s" % (code, self.error()))
		return h


def _pcm16(pcm, n):
	floats = (ctypes.c_float * n).from_address(ctypes.addressof(pcm.contents))
	return array("h", [32767 if x >= 1.0 else -32767 if x <= -1.0 else int(x * 32767.0) for x in floats]).tobytes()


class SynthDriver(BaseSynthDriver):
	name = "st"
	description = "ST"
	supportedSettings = (BaseSynthDriver.VoiceSetting(), BaseSynthDriver.RateSetting())
	supportedCommands = {IndexCommand, BreakCommand}
	supportedNotifications = {synthIndexReached, synthDoneSpeaking}

	@classmethod
	def check(cls):
		return _st_home() is not None

	def __init__(self):
		super().__init__()
		self._home = _st_home()
		self._lib = _Lib(self._home)
		self._neural_ok = os.path.isdir(os.path.join(self._home, "neural"))
		self._handles = {}  # voice id -> engine handle (created lazily, kept: neural loads once)
		self._voice = "fr:ff_siwis" if self._neural_ok else "fr:female"
		self._rate = 50
		self._generation = 0
		self._current = None
		self._lock = threading.Lock()
		self._queue = queue.Queue()
		self._player = self._make_player()
		self._thread = threading.Thread(target=self._run, name="stSynth", daemon=True)
		self._thread.start()
		self._queue.put(("warm", self._voice))

	@staticmethod
	def _make_player():
		try:
			return nvwave.WavePlayer(channels=1, samplesPerSec=RATE, bitsPerSample=16, purpose=nvwave.AudioPurpose.SPEECH)
		except (AttributeError, TypeError):
			import config
			return nvwave.WavePlayer(channels=1, samplesPerSec=RATE, bitsPerSample=16, outputDevice=config.conf["speech"]["outputDevice"])

	# Settings -------------------------------------------------------------
	def _get_availableVoices(self):
		voices = OrderedDict()
		for table in ((NEURAL, COMPACT) if self._neural_ok else (COMPACT,)):
			for vid, (name, lang) in table.items():
				voices[vid] = VoiceInfo(vid, name, lang)
		return voices

	def _get_voice(self):
		return self._voice

	def _set_voice(self, value):
		if value in self.availableVoices:
			self._voice = value
			self._queue.put(("warm", value))  # load the engine now, not at the first key press

	def _get_rate(self):
		return self._rate

	def _set_rate(self, value):
		self._rate = max(0, min(100, int(value)))
		for vid, h in list(self._handles.items()):
			self._lib.dll.st_engine_set_rate_v1(h, self._st_rate(vid))

	def _st_rate(self, vid):
		# NVDA 0..100 -> ST 50..200 (neural ceiling); 50 -> 100 %.
		return int(50 + self._rate * 1.5) if vid in NEURAL else int(50 + self._rate * 2.5)

	def _handle(self, vid):
		h = self._handles.get(vid)
		if h is None:
			lang, voice = vid.split(":", 1)
			try:
				h = self._lib.create("neural" if vid in NEURAL else "compact", lang, voice, self._st_rate(vid))
			except RuntimeError:
				if vid not in NEURAL:
					raise
				log.error("ST neural voice unavailable, using compact", exc_info=True)
				return self._handle(lang + ":female")
			self._handles[vid] = h
		return h

	# Speech ---------------------------------------------------------------
	def speak(self, speechSequence):
		items = []
		for item in speechSequence:
			if isinstance(item, str):
				items.append(("text", item))
			elif isinstance(item, IndexCommand):
				items.append(("index", item.index))
			elif isinstance(item, BreakCommand):
				items.append(("break", item.time))
		with self._lock:
			generation = self._generation
		self._queue.put((generation, self._voice, items))

	def cancel(self):
		with self._lock:
			self._generation += 1
			current = self._current
		kept = []
		try:
			while True:
				job = self._queue.get_nowait()
				if job and job[0] == "warm":
					kept.append(job)  # preloads survive a cancel
		except queue.Empty:
			pass
		for job in kept:
			self._queue.put(job)
		if current is not None:
			self._lib.dll.st_engine_cancel_v1(current)
		self._player.stop()

	def pause(self, switch):
		self._player.pause(switch)

	def terminate(self):
		self.cancel()
		self._queue.put(None)
		self._thread.join(5)
		for h in self._handles.values():
			self._lib.dll.st_engine_destroy_v1(h)
		self._handles.clear()
		self._player.close()

	def _run(self):
		while True:
			job = self._queue.get()
			if job is None:
				return
			if job[0] == "warm":
				try:
					self._handle(job[1])
				except Exception:
					log.error("ST voice preload failed", exc_info=True)
				continue
			generation, vid, items = job
			try:
				self._render(generation, vid, items)
			except Exception:
				log.error("ST synthesis failed", exc_info=True)

	def _live(self, generation):
		return generation == self._generation

	def _render(self, generation, vid, items):
		h = self._handle(vid)
		text = []

		def flush():
			if not text or not self._live(generation):
				text.clear()
				return
			data = " ".join(text).encode("utf-8")
			text.clear()
			if not any(c.isalnum() for c in data.decode("utf-8")):
				return

			def on_audio(pcm, n, rate, _user):
				if not self._live(generation):
					return 0
				self._player.feed(_pcm16(pcm, n))
				return 1

			with self._lock:
				if not self._live(generation):
					return
				self._current = h
			code = self._lib.dll.st_engine_stream_v1(h, data, len(data), _CALLBACK(on_audio), None)
			with self._lock:
				self._current = None
			if code not in (0, 4):
				log.error("ST stream failed (%d): %s" % (code, self._lib.error()))

		for kind, value in items:
			if not self._live(generation):
				return
			if kind == "text":
				text.append(value)
			elif kind == "break":
				flush()
				if not self._live(generation):
					return
				self._player.feed(b"\0\0" * int(RATE * value / 1000))
			elif kind == "index":
				flush()
				if not self._live(generation):
					return  # a cancelled utterance must not report progress
				self._player.feed(b"\0\0" * 2, onDone=lambda i=value: synthIndexReached.notify(synth=self, index=i))
		flush()
		if self._live(generation):
			self._player.idle()
			synthDoneSpeaking.notify(synth=self)
