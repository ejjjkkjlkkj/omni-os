import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

from omni import ceiling, cli


class CliExitStatusTests(unittest.TestCase):
    def _run_hardware_boundary(self, manifest: dict[str, str]) -> int:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "hardware.json"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with patch("sys.argv", ["omni", "hardware-boundary", str(path)]):
                with redirect_stdout(io.StringIO()):
                    return cli.main()

    def test_incomplete_hardware_boundary_exits_nonzero(self):
        self.assertEqual(self._run_hardware_boundary({}), 1)

    def test_complete_hardware_boundary_exits_zero(self):
        manifest = {gate: "PASS" for gate in ceiling.HARDWARE_GATES}
        self.assertEqual(self._run_hardware_boundary(manifest), 0)

    def test_voice_normalize_cli_outputs_renderer_neutral_tokens(self):
        stdout = io.StringIO()
        with patch(
            "sys.argv",
            ["omni", "voice-normalize", "USB 8192 ?", "--lang", "fr"],
        ):
            with redirect_stdout(stdout):
                rc = cli.main()
        self.assertEqual(rc, 0)
        payload = json.loads(stdout.getvalue())
        self.assertEqual(payload[0], {"kind": "acronym", "text": "U S B"})
        self.assertEqual(
            payload[1],
            {"kind": "number", "text": "huit mille cent quatre-vingt-douze"},
        )
        self.assertEqual(payload[2], {"kind": "clause", "text": "question"})

    def test_voice_compile_cli_outputs_versioned_stream(self):
        stdout = io.StringIO()
        with patch(
            "sys.argv",
            ["omni", "voice-compile", "USB 8192 ?", "--lang", "fr"],
        ):
            with redirect_stdout(stdout):
                rc = cli.main()
        self.assertEqual(rc, 0)
        payload = json.loads(stdout.getvalue())
        self.assertEqual(payload["version"], 1)
        self.assertTrue(payload["bytes"])

    def test_voice_pcm_check_rejects_silence(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "silence.pcm"
            path.write_bytes(bytes(512))
            stdout = io.StringIO()
            with patch("sys.argv", ["omni", "voice-pcm-check", str(path)]):
                with redirect_stdout(stdout):
                    rc = cli.main()
        self.assertEqual(rc, 1)
        payload = json.loads(stdout.getvalue())
        self.assertEqual(payload["status"], "VOICE_PCM_REJECTED")
        self.assertIn("silence-or-near-silence-channel-0", payload["violations"])


if __name__ == "__main__":
    unittest.main()
