"""Turn one agent-cycle result into fail-closed evidence."""
from __future__ import annotations

from evidence_ledger import record


def record_cycle(commit: str, verified: bool, blockers: list[str], tool_count: int):
    if blockers:
        status = "BLOCKED"
    elif verified:
        status = "PASS"
    else:
        status = "FAIL"
    return record(
        "agent-cycle",
        status,
        "continuous-cycle",
        {"commit": commit, "blockers": blockers, "tool_count": tool_count},
    )
