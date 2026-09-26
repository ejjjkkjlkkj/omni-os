"""Screen-reader announcements: semantic events -> interruptible speech.

``announcement`` turns a semantic ``Node`` into the sentence a screen reader
speaks (name, role, state, value), in French or English. ``SpeechController``
consumes the same events as ``SemanticModel`` and drives any renderer with
``speak(text, sink)`` / ``cancel()`` (``voice_st.StRenderer`` in production):
a focus change interrupts the current utterance at once, so speech always
describes the latest focus. Rendering runs on one worker thread; the caller
(event loop, keyboard hook) never blocks on synthesis.
"""
from __future__ import annotations

import queue
import threading
from collections.abc import Callable
from typing import Protocol

from .semantic import Node, SemanticModel
from .voice_frontend import Language
from .voice_st import StCancelled

ROLE_NAMES = {
    Language.FR: {
        "menuitem": "élément de menu", "button": "bouton", "checkbox": "case à cocher",
        "radiobutton": "bouton radio", "slider": "curseur", "edit": "zone d'édition",
        "listitem": "élément de liste", "tab": "onglet", "bootentry": "entrée de démarrage",
    },
    Language.EN: {
        "menuitem": "menu item", "button": "button", "checkbox": "checkbox",
        "radiobutton": "radio button", "slider": "slider", "edit": "edit",
        "listitem": "list item", "tab": "tab", "bootentry": "boot entry",
    },
}
STATE_NAMES = {
    Language.FR: {"checked": "cochée", "unchecked": "non cochée", "selected": "sélectionné",
                  "disabled": "indisponible", "expanded": "développé", "collapsed": "réduit",
                  "readonly": "lecture seule"},
    Language.EN: {"checked": "checked", "unchecked": "not checked", "selected": "selected",
                  "disabled": "unavailable", "expanded": "expanded", "collapsed": "collapsed",
                  "readonly": "read only"},
}
_STATE_ORDER = ("checked", "unchecked", "selected", "expanded", "collapsed", "readonly", "disabled")
_PROTECTED = {Language.FR: "protégé", Language.EN: "protected"}


def announcement(node: Node, language: Language | str = Language.FR) -> str:
    """Name, role, states, value. Password values are never spoken."""
    lang = Language(language)
    state = {s.lower() for s in node.state}
    parts = [node.name.strip()] if node.name.strip() else []
    role = ROLE_NAMES[lang].get(node.role.lower())
    if role:
        parts.append(role)
    if node.role.lower() == "checkbox" and "checked" not in state:
        state.add("unchecked")
    parts += [STATE_NAMES[lang][s] for s in _STATE_ORDER if s in state]
    if "password" in state:
        if node.value:
            parts.append(_PROTECTED[lang])
    elif node.value.strip():
        parts.append(node.value.strip())
    return ", ".join(parts) + "." if parts else ""


class Renderer(Protocol):
    def speak(self, text: str, sink: Callable[[memoryview], bool | None]) -> object: ...
    def cancel(self) -> None: ...


class SpeechController:
    """Apply semantic events and speak what the user needs to hear.

    ``sink`` receives float32 48 kHz chunks (audio device). Utterances are
    queued; ``interrupt`` (and every focus change) drops the queue and cancels
    the utterance being rendered or played.
    """

    def __init__(self, renderer: Renderer, sink: Callable[[memoryview], bool | None],
                 language: Language | str = Language.FR, on_error: Callable[[BaseException], None] | None = None):
        self.model = SemanticModel()
        self.language = Language(language)
        self._renderer, self._sink, self._on_error = renderer, sink, on_error
        self._queue: queue.Queue[str | None] = queue.Queue()
        self._generation = 0
        self._lock = threading.Lock()
        self.spoken: list[str] = []  # completed utterances, for tests and evidence
        self._thread = threading.Thread(target=self._run, name="omni-speech", daemon=True)
        self._thread.start()

    def say(self, text: str) -> None:
        if text:
            self._queue.put(text)

    def interrupt(self) -> None:
        with self._lock:
            self._generation += 1
            while True:
                try:
                    self._queue.get_nowait()
                except queue.Empty:
                    break
        self._renderer.cancel()

    def apply(self, event: dict[str, object]) -> None:
        self.model.apply(event)
        kind = event.get("kind")
        focus = self.model.nodes.get(self.model.focus) if self.model.focus is not None else None
        if kind == "focus_changed" and focus is not None:
            self.interrupt()
            self.say(announcement(focus, self.language))
        elif kind in ("value_changed", "state_changed") and focus is not None and self._event_node(event) == focus.id:
            self.interrupt()
            if kind == "value_changed" and "password" not in focus.state:
                self.say(focus.value.strip() + ".")
            else:
                self.say(announcement(focus, self.language))

    def _event_node(self, event: dict[str, object]) -> int:
        raw = event.get("node")
        return self.model._node_id(event, raw if isinstance(raw, dict) else {})

    def wait_idle(self, timeout: float = 30.0) -> bool:
        done = threading.Event()
        self._queue.put(done)  # type: ignore[arg-type]
        return done.wait(timeout)

    def close(self) -> None:
        self.interrupt()
        self._queue.put(None)
        self._thread.join(5)

    def _run(self) -> None:
        while (item := self._queue.get()) is not None:
            if isinstance(item, threading.Event):
                item.set()
                continue
            with self._lock:
                generation = self._generation
            try:
                self._renderer.speak(item, lambda chunk: generation == self._generation and self._sink(chunk) is not False)
            except StCancelled:
                continue
            except Exception as e:  # a renderer failure must not kill the screen reader
                if self._on_error:
                    self._on_error(e)
                continue
            if generation == self._generation:
                self.spoken.append(item)
