from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterable

FOCUSABLE_ROLES = {
    "menuitem",
    "button",
    "checkbox",
    "radiobutton",
    "slider",
    "edit",
    "listitem",
    "tab",
    "bootentry",
}


@dataclass(slots=True)
class Node:
    id: int
    role: str
    name: str
    parent_id: int | None = None
    value: str = ""
    state: set[str] = field(default_factory=set)


class SemanticModel:
    def __init__(self) -> None:
        self.nodes: dict[int, Node] = {}
        self.focus: int | None = None
        self.sequence = -1
        self.violations: list[dict[str, object]] = []

    def _violate(self, code: str, **detail: object) -> None:
        self.violations.append({"code": code, **detail})

    def _validate(self, node: Node) -> None:
        role = node.role.lower()
        state = {item.lower() for item in node.state}
        if not role or role == "unknown":
            self._violate("MISSING_ROLE", node=node.id)
        if role in FOCUSABLE_ROLES and not node.name.strip():
            self._violate("FOCUSABLE_WITHOUT_NAME", node=node.id)
        if "password" in state and node.value:
            self._violate("PASSWORD_VALUE_EXPOSED", node=node.id)
        if node.parent_id == node.id:
            self._violate("SELF_PARENT", node=node.id)

    @staticmethod
    def _node_id(event: dict[str, object], raw: dict[str, object]) -> int:
        source = raw.get("id", event.get("node_id", -1))
        try:
            value = int(source)
        except (TypeError, ValueError):
            return -1
        return value if value >= 0 else -1

    @staticmethod
    def _state(raw: object) -> set[str]:
        if raw is None:
            return set()
        if isinstance(raw, str):
            return {raw.strip().lower()} if raw.strip() else set()
        return {str(item).strip().lower() for item in raw if str(item).strip()}

    def _new_node(self, node_id: int, raw: dict[str, object]) -> Node:
        parent = raw.get("parent_id")
        parent_id = None if parent is None else int(parent)
        return Node(
            id=node_id,
            parent_id=parent_id,
            role=str(raw.get("role", "unknown")).strip().lower(),
            name=str(raw.get("name", "")),
            value=str(raw.get("value", "")),
            state=self._state(raw.get("state", [])),
        )

    def _patch_node(self, node: Node, raw: dict[str, object]) -> None:
        if "parent_id" in raw:
            parent = raw["parent_id"]
            node.parent_id = None if parent is None else int(parent)
        if "role" in raw:
            node.role = str(raw["role"]).strip().lower()
        if "name" in raw:
            node.name = str(raw["name"])
        if "value" in raw:
            node.value = str(raw["value"])
        if "state" in raw:
            node.state = self._state(raw["state"])

    def _check_focus_invariants(self) -> None:
        focused = [
            node.id
            for node in self.nodes.values()
            if "focused" in {state.lower() for state in node.state}
        ]
        if len(focused) > 1:
            self._violate("MULTIPLE_FOCUS", nodes=focused)
        if self.focus is not None:
            if self.focus not in self.nodes:
                self._violate("FOCUS_POINTS_TO_UNKNOWN_NODE", node=self.focus)
            elif self.focus not in focused:
                self._violate("FOCUS_STATE_DESYNC", node=self.focus)

    def apply(self, event: dict[str, object]) -> None:
        if not isinstance(event, dict):
            self._violate("INVALID_EVENT")
            return
        try:
            sequence = int(event["sequence"])
        except (KeyError, TypeError, ValueError):
            self._violate("INVALID_SEQUENCE", value=event.get("sequence"))
            return

        if sequence <= self.sequence:
            self._violate(
                "NON_MONOTONIC_SEQUENCE",
                sequence=sequence,
                previous=self.sequence,
            )
        self.sequence = max(self.sequence, sequence)

        kind = str(event.get("kind", "")).strip()
        raw_value = event.get("node")
        if raw_value is None:
            raw: dict[str, object] = {}
        elif isinstance(raw_value, dict):
            raw = dict(raw_value)
        else:
            self._violate("INVALID_NODE_PAYLOAD", kind=kind)
            self._check_focus_invariants()
            return

        node_id = self._node_id(event, raw)

        if kind == "node_created":
            if node_id < 0:
                self._violate("MISSING_NODE_ID", kind=kind)
            elif node_id in self.nodes:
                self._violate("DUPLICATE_NODE_CREATE", node=node_id)
            else:
                try:
                    node = self._new_node(node_id, raw)
                except (TypeError, ValueError):
                    self._violate("INVALID_NODE_PAYLOAD", node=node_id)
                else:
                    self.nodes[node_id] = node
                    self._validate(node)

        elif kind == "node_updated":
            node = self.nodes.get(node_id)
            if node is None:
                self._violate("UPDATE_UNKNOWN_NODE", node=node_id)
            else:
                try:
                    self._patch_node(node, raw)
                except (TypeError, ValueError):
                    self._violate("INVALID_NODE_PAYLOAD", node=node_id)
                else:
                    self._validate(node)

        elif kind == "value_changed":
            node = self.nodes.get(node_id)
            if node is None:
                self._violate("VALUE_UNKNOWN_NODE", node=node_id)
            elif "value" not in raw:
                self._violate("VALUE_MISSING", node=node_id)
            else:
                node.value = str(raw["value"])
                self._validate(node)

        elif kind == "state_changed":
            node = self.nodes.get(node_id)
            if node is None:
                self._violate("STATE_UNKNOWN_NODE", node=node_id)
            elif "state" not in raw:
                self._violate("STATE_MISSING", node=node_id)
            else:
                try:
                    node.state = self._state(raw["state"])
                except TypeError:
                    self._violate("INVALID_NODE_PAYLOAD", node=node_id)
                else:
                    self._validate(node)

        elif kind == "focus_changed":
            node = self.nodes.get(node_id)
            if node is None:
                self._violate("FOCUS_UNKNOWN_NODE", node=node_id)
            else:
                try:
                    self._patch_node(node, raw)
                except (TypeError, ValueError):
                    self._violate("INVALID_NODE_PAYLOAD", node=node_id)
                else:
                    self.focus = node_id
                    for current_id, current in self.nodes.items():
                        if current_id == node_id:
                            current.state.add("focused")
                        else:
                            current.state.discard("focused")
                    self._validate(node)

        elif kind == "node_removed":
            if node_id not in self.nodes:
                self._violate("REMOVE_UNKNOWN_NODE", node=node_id)
            else:
                self.nodes.pop(node_id)
                if self.focus == node_id:
                    self.focus = None

        else:
            self._violate("UNKNOWN_EVENT_KIND", kind=kind)

        self._check_focus_invariants()

    def apply_all(self, events: Iterable[dict[str, object]]) -> None:
        for event in events:
            self.apply(event)

    @property
    def passed(self) -> bool:
        return not self.violations
