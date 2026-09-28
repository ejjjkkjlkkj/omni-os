#!/usr/bin/env python3
"""Drive a running QEMU UEFI screen reader through QMP/HMP sendkey.

The scenario is intentionally non-destructive:
F1 help -> Down -> Up -> Escape.

Default mode proves rapid keyboard delivery and speech interruption. With
--wait-speech-complete, each spoken navigation event is allowed to finish before
the next key so the captured WAV can be validated bit-for-bit.
"""
from __future__ import annotations

import argparse
import json
import socket
import time
from pathlib import Path


def wait_for(predicate, timeout: float, label: str) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for {label}")


class QMP:
    def __init__(self, path: Path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(str(path))
        self.file = self.sock.makefile("rwb", buffering=0)
        greeting = self._read_message()
        if "QMP" not in greeting:
            raise RuntimeError(f"unexpected QMP greeting: {greeting!r}")
        self.execute("qmp_capabilities")

    def _read_message(self) -> dict:
        while True:
            line = self.file.readline()
            if not line:
                raise RuntimeError("QMP socket closed")
            msg = json.loads(line.decode("utf-8"))
            if "event" in msg:
                continue
            return msg

    def execute(self, command: str, arguments: dict | None = None) -> dict:
        request = {"execute": command}
        if arguments is not None:
            request["arguments"] = arguments
        self.file.write((json.dumps(request) + "\n").encode("utf-8"))
        response = self._read_message()
        if "error" in response:
            raise RuntimeError(f"QMP {command} failed: {response['error']}")
        return response.get("return", {})

    def hmp(self, command_line: str) -> str:
        value = self.execute(
            "human-monitor-command", {"command-line": command_line}
        )
        return value if isinstance(value, str) else json.dumps(value)

    def close(self) -> None:
        try:
            self.file.close()
        finally:
            self.sock.close()


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="ignore")
    except FileNotFoundError:
        return ""


def contains(path: Path, marker: str) -> bool:
    return marker in read_text(path)


def count_marker(path: Path, marker: str) -> int:
    return read_text(path).count(marker)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qmp", required=True, type=Path)
    parser.add_argument("--serial", required=True, type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--delay", type=float, default=0.75)
    parser.add_argument("--wait-speech-complete", action="store_true")
    parser.add_argument("--speech-timeout", type=float, default=45.0)
    args = parser.parse_args()

    wait_for(args.qmp.exists, args.timeout, "QMP socket")
    wait_for(
        lambda: contains(args.serial, "HII_GRAPH_NAV_READY=PASS"),
        args.timeout,
        "UEFI navigation ready marker",
    )

    qmp = QMP(args.qmp)
    scenario = [
        ("f1", "HII_GRAPH_NAV_KEY=F1"),
        ("down", "HII_GRAPH_NAV_KEY=DOWN"),
        ("up", "HII_GRAPH_NAV_KEY=UP"),
        ("esc", "HII_GRAPH_NAV_KEY=ESC"),
    ]
    evidence: list[dict[str, str]] = []
    try:
        for key, marker in scenario:
            completed_before = count_marker(
                args.serial, "HII_GRAPH_SPEECH_PHRASE_COMPLETE=PASS"
            )
            qmp.hmp(f"sendkey {key}")
            wait_for(
                lambda marker=marker: contains(args.serial, marker),
                5.0,
                marker,
            )
            item = {"key": key, "marker": marker, "status": "PASS"}
            evidence.append(item)
            print(f"QEMU_KEY_{key.upper()}=PASS")

            if args.wait_speech_complete and key != "esc":
                wait_for(
                    lambda before=completed_before: count_marker(
                        args.serial, "HII_GRAPH_SPEECH_PHRASE_COMPLETE=PASS"
                    )
                    > before,
                    args.speech_timeout,
                    f"completed speech after {key}",
                )
                item["speech_complete"] = "PASS"
                print(f"QEMU_SPEECH_{key.upper()}_COMPLETE=PASS")
                # The HDA IOC proves the guest consumed the whole DMA buffer,
                # but QEMU's WAV backend may still have host-side frames to
                # flush. Preserve a small capture margin before the next key
                # (especially Escape/quit after the final utterance).
                time.sleep(args.delay)
            else:
                time.sleep(args.delay)

        wait_for(
            lambda: contains(args.serial, "HII_GRAPH_NAV_EXIT=PASS"),
            5.0,
            "navigation exit marker",
        )
        print("NAVIGATION_INPUT=PASS")
        print("NAVIGATION_RUNTIME=PASS")
        if args.wait_speech_complete:
            print("NAVIGATION_COMPLETE_SPEECH_SCENARIO=PASS")
        try:
            qmp.execute("quit")
        except RuntimeError:
            # QEMU may close the socket before returning the quit response.
            pass
    finally:
        qmp.close()

    report = args.report or (args.serial.parent / "qemu-navigation-report.json")
    report.write_text(
        json.dumps(
            {
                "scenario": evidence,
                "wait_speech_complete": args.wait_speech_complete,
                "status": "PASS",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
