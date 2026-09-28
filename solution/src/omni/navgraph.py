from __future__ import annotations

from collections import deque


def _result(
    *,
    nodes: int,
    reachable: int,
    violations: list[dict[str, object]],
) -> dict[str, object]:
    coverage = 100.0 * reachable / nodes if nodes else 0.0
    return {
        "status": "PASS" if not violations else "FAIL",
        "nodes": nodes,
        "reachable": reachable,
        "coverage_percent": coverage,
        "violations": violations,
    }


def check_graph(graph: dict[str, object]) -> dict[str, object]:
    violations: list[dict[str, object]] = []
    raw_nodes = graph.get("nodes", [])
    if not isinstance(raw_nodes, list):
        return _result(
            nodes=0,
            reachable=0,
            violations=[{"code": "NODES_NOT_LIST"}],
        )

    nodes: dict[str, dict[str, object]] = {}
    first_index: dict[str, int] = {}
    for index, raw in enumerate(raw_nodes):
        if not isinstance(raw, dict):
            violations.append({"code": "NODE_NOT_OBJECT", "index": index})
            continue
        if "id" not in raw:
            violations.append({"code": "NODE_MISSING_ID", "index": index})
            continue

        node_id = str(raw["id"]).strip()
        if not node_id:
            violations.append({"code": "NODE_EMPTY_ID", "index": index})
            continue
        if node_id in nodes:
            violations.append(
                {
                    "code": "DUPLICATE_NODE_ID",
                    "node": node_id,
                    "first_index": first_index[node_id],
                    "duplicate_index": index,
                }
            )
            continue

        nodes[node_id] = raw
        first_index[node_id] = index

    start = str(graph.get("start", "")).strip()
    start_valid = start in nodes
    if not start_valid:
        violations.append({"code": "MISSING_START", "start": start})

    valid_edges: dict[str, list[tuple[str, str]]] = {node_id: [] for node_id in nodes}
    for node_id, node in nodes.items():
        edges = node.get("edges", [])
        if not isinstance(edges, list):
            violations.append({"code": "EDGES_NOT_LIST", "node": node_id})
            continue

        seen: dict[str, str] = {}
        for edge_index, edge in enumerate(edges):
            if not isinstance(edge, dict):
                violations.append(
                    {
                        "code": "EDGE_NOT_OBJECT",
                        "node": node_id,
                        "edge_index": edge_index,
                    }
                )
                continue

            action = str(edge.get("action", "")).strip()
            target = str(edge.get("to", "")).strip()
            if not action:
                violations.append(
                    {
                        "code": "EDGE_MISSING_ACTION",
                        "node": node_id,
                        "edge_index": edge_index,
                    }
                )
            if not target:
                violations.append(
                    {
                        "code": "EDGE_MISSING_TARGET",
                        "node": node_id,
                        "edge_index": edge_index,
                    }
                )
            if not action or not target:
                continue

            if target not in nodes:
                violations.append(
                    {
                        "code": "EDGE_TO_UNKNOWN",
                        "node": node_id,
                        "action": action,
                        "target": target,
                    }
                )

            previous = seen.get(action)
            if previous is not None and previous != target:
                violations.append(
                    {
                        "code": "NONDETERMINISTIC_ACTION",
                        "node": node_id,
                        "action": action,
                        "targets": [previous, target],
                    }
                )
            else:
                seen[action] = target

            valid_edges[node_id].append((action, target))

        if bool(node.get("focusable", False)) and not str(node.get("name", "")).strip():
            violations.append({"code": "FOCUSABLE_WITHOUT_NAME", "node": node_id})
        if bool(node.get("password", False)) and str(node.get("value", "")):
            violations.append({"code": "PASSWORD_VALUE_EXPOSED", "node": node_id})

    if not start_valid:
        return _result(nodes=len(nodes), reachable=0, violations=violations)

    reachable = {start}
    queue = deque([start])
    while queue:
        current = queue.popleft()
        for _, target in valid_edges[current]:
            if target in nodes and target not in reachable:
                reachable.add(target)
                queue.append(target)

    for node_id in nodes:
        if node_id not in reachable:
            violations.append({"code": "UNREACHABLE_NODE", "node": node_id})

    reverse: dict[str, list[str]] = {}
    for node_id, edges in valid_edges.items():
        for _, target in edges:
            if target in nodes:
                reverse.setdefault(target, []).append(node_id)

    can_return = {start}
    queue = deque([start])
    while queue:
        current = queue.popleft()
        for previous in reverse.get(current, []):
            if previous not in can_return:
                can_return.add(previous)
                queue.append(previous)

    for node_id in sorted(reachable):
        node = nodes[node_id]
        if bool(node.get("terminal", False)):
            continue
        if node_id not in can_return:
            violations.append({"code": "NO_PATH_BACK_TO_START", "node": node_id})
        if bool(node.get("focusable", False)) and not valid_edges[node_id]:
            violations.append({"code": "FOCUS_DEAD_END", "node": node_id})

    return _result(
        nodes=len(nodes),
        reachable=len(reachable),
        violations=violations,
    )
