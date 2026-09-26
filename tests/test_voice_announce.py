import os
import threading
import time
import unittest

from omni.announce import SpeechController, announcement
from omni.semantic import Node
from omni.voice_st import StCancelled


class FakeRenderer:
    """Deterministic renderer: 10 chunks of 5 ms per utterance, cancellable."""

    def __init__(self):
        self._cancel = threading.Event()
        self.started: list[str] = []

    def speak(self, text, sink):
        self._cancel.clear()
        self.started.append(text)
        for _ in range(10):
            time.sleep(0.005)
            if self._cancel.is_set() or sink(memoryview(b"\0" * 16).cast("f")) is False:
                raise StCancelled(4, "cancelled")
        return text

    def cancel(self):
        self._cancel.set()


def focus(seq, node_id, role, name, **extra):
    return {"sequence": seq, "kind": "focus_changed", "node": {"id": node_id, "role": role, "name": name, **extra}}


def create(seq, node_id, role, name, **extra):
    return {"sequence": seq, "kind": "node_created", "node": {"id": node_id, "role": role, "name": name, **extra}}


class AnnouncementTests(unittest.TestCase):
    def test_french_and_english_roles_and_states(self):
        box = Node(1, "checkbox", "Démarrage sécurisé", state={"checked"})
        self.assertEqual(announcement(box, "fr"), "Démarrage sécurisé, case à cocher, cochée.")
        box.state = set()
        self.assertEqual(announcement(box, "en"), "Démarrage sécurisé, checkbox, not checked.")
        entry = Node(2, "bootentry", "Windows Boot Manager", state={"selected", "disabled"})
        self.assertEqual(announcement(entry, "fr"), "Windows Boot Manager, entrée de démarrage, sélectionné, indisponible.")

    def test_password_value_is_never_spoken(self):
        pw = Node(3, "edit", "Mot de passe", value="hunter2", state={"password"})
        text = announcement(pw, "fr")
        self.assertNotIn("hunter2", text)
        self.assertIn("protégé", text)

    def test_secret_fields_never_speak_their_value(self):
        from omni.announce import is_secret
        cases = [
            Node(10, "edit", "Mot de passe", value="hunter2", state={"password"}),
            Node(11, "pin", "Code", value="1234"),
            Node(12, "edit", "Code PIN", value="0000"),               # no flag, name says PIN
            Node(13, "edit", "Password", value="s3cr3t!"),            # no flag, English name
            Node(14, "edit", "Clé", value="abcd", state={"secure"}),
            Node(15, "credential", "Identifiants", value="bob:pw"),
            Node(16, "edit", "Cryptogramme visuel", value="123"),
        ]
        for node in cases:
            self.assertTrue(is_secret(node), node)
            for lang in ("fr", "en"):
                text = announcement(node, lang)
                self.assertNotIn(node.value, text, (node, lang))
        self.assertEqual(announcement(cases[0], "fr"), "Mot de passe, zone d'édition, protégé, 7 caractères.")
        self.assertFalse(is_secret(Node(20, "edit", "Nom d'utilisateur", value="bob")))
        self.assertFalse(is_secret(Node(21, "button", "Afficher le mot de passe")))

    def test_value_is_spoken(self):
        self.assertEqual(announcement(Node(4, "slider", "Volume", value="40 %"), "fr"), "Volume, curseur, 40 %.")


class SpeechControllerTests(unittest.TestCase):
    def setUp(self):
        self.renderer = FakeRenderer()
        self.chunks = 0

        def sink(chunk):
            self.chunks += 1
        self.controller = SpeechController(self.renderer, sink, "fr")

    def tearDown(self):
        self.controller.close()

    def test_rapid_focus_changes_speak_only_the_latest(self):
        c = self.controller
        c.apply(create(0, 1, "menuitem", "Boot"))
        c.apply(create(1, 2, "menuitem", "Security"))
        c.apply(create(2, 3, "menuitem", "Exit"))
        c.apply(focus(3, 1, "menuitem", "Boot"))
        time.sleep(0.012)  # user keeps arrowing while the first item is speaking
        c.apply(focus(4, 2, "menuitem", "Security"))
        c.apply(focus(5, 3, "menuitem", "Exit"))
        self.assertTrue(c.wait_idle())
        self.assertEqual(c.spoken, ["Exit, élément de menu."])
        self.assertTrue(c.model.passed, c.model.violations)

    def test_value_change_on_focused_node_is_announced(self):
        c = self.controller
        c.apply(create(0, 1, "slider", "Volume", value="40"))
        c.apply(focus(1, 1, "slider", "Volume"))
        c.apply({"sequence": 2, "kind": "value_changed", "node": {"id": 1, "value": "50"}})
        self.assertTrue(c.wait_idle())
        self.assertEqual(c.spoken[-1], "50.")

    def test_typing_in_secret_field_is_silent_and_unlogged(self):
        c = self.controller
        c.apply(create(0, 1, "edit", "Mot de passe", state=["password"]))
        c.apply(focus(1, 1, "edit", "Mot de passe", state=["password"]))
        for i, secret in enumerate(["h", "hu", "hun", "hunter2"]):
            c.apply({"sequence": 2 + i, "kind": "value_changed", "node": {"id": 1, "value": secret}})
        self.assertTrue(c.wait_idle())
        self.assertEqual(c.spoken, ["Mot de passe, zone d'édition, protégé."])
        self.assertTrue(all("hunter" not in text for text in self.renderer.started))

    def test_interrupt_stops_the_audio_device(self):
        stops = []

        class Sink:
            def __call__(self, chunk):
                return True

            def stop(self):
                stops.append(1)
        c = SpeechController(FakeRenderer(), Sink(), "fr")
        c.interrupt()
        c.close()
        self.assertGreaterEqual(len(stops), 1)

    def test_renderer_errors_are_reported_not_fatal(self):
        errors = []

        class Broken(FakeRenderer):
            def speak(self, text, sink):
                raise RuntimeError("device lost")
        c = SpeechController(Broken(), lambda chunk: None, "en", on_error=errors.append)
        c.say("Hello.")
        self.assertTrue(c.wait_idle())
        c.close()
        self.assertEqual([str(e) for e in errors], ["device lost"])


@unittest.skipUnless(os.environ.get("OMNI_ST_HOME") or os.environ.get("OMNI_ST_LIB"), "needs an ST release")
class SpeechControllerWithStTests(unittest.TestCase):
    def test_focus_navigation_with_st(self):
        from omni.voice_st import StRenderer
        backend = os.environ.get("OMNI_ST_TEST_BACKEND", "neural")
        with StRenderer(lang="fr", backend=backend) as renderer:
            samples = []
            c = SpeechController(renderer, lambda chunk: samples.append(len(chunk)), "fr")
            c.apply(create(0, 1, "bootentry", "Windows Boot Manager"))
            c.apply(create(1, 2, "checkbox", "Démarrage sécurisé", state=["checked"]))
            c.apply(focus(2, 1, "bootentry", "Windows Boot Manager"))
            c.apply(focus(3, 2, "checkbox", "Démarrage sécurisé"))
            self.assertTrue(c.wait_idle(60))
            c.close()
        self.assertEqual(c.spoken, ["Démarrage sécurisé, case à cocher, cochée."])
        self.assertGreater(sum(samples), 48000 // 2)


if __name__ == "__main__":
    unittest.main()
