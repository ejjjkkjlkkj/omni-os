#!/usr/bin/env python3
"""Drive a running QEMU with real key presses over QMP (TCP), step by step.

Each step waits for a NEW occurrence of a proof marker in the debug log (one more than when
the previous step finished), then presses keys:

    qmp-keys.py PORT LOG TIMEOUT "MARKER|key key ..." ["MARKER|key ..." ...]

A step with no keys only waits for its marker. Exit 0 when every step completed.
"""
from __future__ import annotations

import json
import socket
import sys
import time
from pathlib import Path


def count(log: Path, marker: str) -> int:
    return log.read_text(encoding="utf-8", errors="replace").count(marker) if log.exists() else 0


def wait_for(log: Path, marker: str, seen: int, deadline: float) -> int:
    while time.monotonic() < deadline:
        now = count(log, marker)
        if now > seen:
            return now
        time.sleep(0.2)
    raise TimeoutError(f"timeout waiting for {marker} (occurrence {seen + 1})")


class Qmp:
    def __init__(self, port: int, deadline: float):
        while True:
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError:
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.2)
        self.file = self.sock.makefile("rwb", buffering=0)
        self._read()  # greeting
        self.command("qmp_capabilities")

    def _read(self) -> dict:
        while True:
            message = json.loads(self.file.readline())
            if "event" not in message:
                return message

    def command(self, name: str, **arguments) -> dict:
        payload = {"execute": name, **({"arguments": arguments} if arguments else {})}
        self.file.write(json.dumps(payload).encode() + b"\n")
        reply = self._read()
        if "error" in reply:
            raise RuntimeError(reply["error"])
        return reply

    def key(self, name: str) -> None:
        self.command("human-monitor-command", **{"command-line": f"sendkey {name}"})


def main(argv: list[str]) -> int:
    port, log, timeout = int(argv[1]), Path(argv[2]), float(argv[3])
    deadline = time.monotonic() + timeout
    qmp = Qmp(port, deadline)
    seen: dict[str, int] = {}
    for step in argv[4:]:
        marker, _, keys = step.partition("|")
        seen[marker] = wait_for(log, marker, seen.get(marker, 0), deadline)
        print(f"STEP {marker}", flush=True)
        for key in keys.split():
            qmp.key(key)
            time.sleep(0.4)
    print("QMP_KEYS=PASS", flush=True)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except (TimeoutError, OSError, RuntimeError) as error:
        print(f"QMP_KEYS=FAIL {error}", flush=True)
        sys.exit(1)
