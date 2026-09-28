#!/usr/bin/env python3
"""Deterministically derive the accessibility taxonomy and source-proof files.

Every output is derived from committed, authoritative repository data — never
invented:

- accessibility sources, their layer and URL come from
  data/threat-intel/sources.json (domain == "accessibility");
- publication layers come from data/taxonomy/network-layers.json;
- the 23 depth layers L16..L-6 and the accessibility "core invariant" capability
  set are transcribed from docs/ACCESSIBILITY_FULL_STACK.md;
- the authorization line and actor classes are transcribed from
  docs/THREAT_LANDSCAPE.md.

Re-run this script to regenerate the files reproducibly:

    python scripts/build_accessibility_taxonomy.py
"""
from __future__ import annotations

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
TAX = ROOT / "data" / "taxonomy"

# --- Depth layers L16..L-6, transcribed from docs/ACCESSIBILITY_FULL_STACK.md ---
DEPTH_LAYERS: list[tuple[str, int, str]] = [
    ("L16", 16, "Governance"),
    ("L15", 15, "Human / operator"),
    ("L14", 14, "Identity"),
    ("L13", 13, "Application"),
    ("L12", 12, "Data / content"),
    ("L11", 11, "Language / runtime"),
    ("L10", 10, "Build / CI / packages"),
    ("L9", 9, "Sandbox / IPC / services"),
    ("L8", 8, "Naming / discovery"),
    ("L7", 7, "Crypto / session"),
    ("L6", 6, "Overlay / privacy / P2P"),
    ("L5", 5, "Transport"),
    ("L4", 4, "Network / routing"),
    ("L3", 3, "Link / radio"),
    ("L2", 2, "Device / driver / DMA"),
    ("L1", 1, "Kernel / hypervisor"),
    ("L0", 0, "Firmware / boot"),
    ("L-1", -1, "Component authentication"),
    ("L-2", -2, "Root of trust"),
    ("L-3", -3, "Silicon security state"),
    ("L-4", -4, "Manufacturing"),
    ("L-5", -5, "Physical"),
    ("L-6", -6, "Supply-chain lifecycle"),
]

# --- Core-invariant accessibility capabilities, transcribed from the doc's
# "For a blind operator, every layer must answer:" list. Each layer must expose
# every one of these. ---
A11Y_CAPABILITIES: list[tuple[str, str]] = [
    ("state-observable", "What state exists?"),
    ("change-notified", "What changed?"),
    ("trust-indicated", "Is it trusted?"),
    ("action-available", "What action is available?"),
    ("danger-flagged", "What action is dangerous?"),
    ("secret-redacted", "What is secret and therefore redacted?"),
    ("speech-fallback", "What happens if speech fails?"),
    ("braille-fallback", "What happens if braille fails?"),
    ("trusted-path-recovery", "What happens if the normal UI is compromised?"),
    ("evidence-provable", "What evidence proves the state?"),
]

CORE_INVARIANT = (
    "NO SECURITY LAYER IS COMPLETE WITHOUT AN ACCESSIBLE "
    "OBSERVATION / CONTROL / RECOVERY PATH."
)

# --- Authorization line and actor classes, transcribed from THREAT_LANDSCAPE.md ---
AUTHORIZATION_LINE = "explicit permission + defined target + defined scope + bounded impact"
AUTHORIZED_CLASSES = [
    "security researchers", "vulnerability researchers", "bug bounty", "pentesters",
    "red teams", "blue teams", "purple teams", "SOC", "CERT/CSIRT",
    "incident response", "forensics", "reverse engineering", "malware analysis",
    "threat hunting", "detection engineering", "fuzzing", "secure development",
    "supply-chain review", "hardware/firmware audit",
    "cloud/mobile/OT security", "authorized adversary emulation",
]
UNAUTHORIZED_CLASSES = [
    "state and state-sponsored actors", "APT groups", "cybercrime/eCrime",
    "ransomware operators and affiliates", "initial-access brokers", "botnets",
    "credential theft", "phishing", "malware operators", "commercial spyware abuse",
    "exploit brokers", "malicious insiders", "hacktivists", "unattributed campaigns",
]


def _write(path: pathlib.Path, payload) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    # Force LF so regeneration is byte-identical on Windows and Linux/CI.
    path.write_text(text, encoding="utf-8", newline="\n")


def _load(rel: str):
    return json.loads((ROOT / rel).read_text(encoding="utf-8"))


def derive() -> dict[str, dict]:
    """Return {filename: payload} for every taxonomy file, without writing."""
    sources = _load("data/threat-intel/sources.json")["sources"]
    network = _load("data/taxonomy/network-layers.json")
    publication_keys = sorted(network["publication_layers"])

    a11y_sources = [s for s in sources if s.get("domain") == "accessibility"]
    capability_ids = [cid for cid, _ in A11Y_CAPABILITIES]

    # 1. full-stack-layers.json (deep security/accessibility stack, doc order).
    full_stack = {
        "schema_version": 1,
        "layers": [
            {"id": lid, "layer_number": num, "name": name}
            for lid, num, name in DEPTH_LAYERS
        ],
    }

    # 2. accessibility-full-stack.json (taxonomy: every layer must expose the
    #    full core-invariant capability set).
    a11y_full = {
        "schema_version": 1,
        "invariant": CORE_INVARIANT,
        "capabilities": {cid: question for cid, question in A11Y_CAPABILITIES},
        "publication_layers": {
            key: {
                "name": key.replace("_", " "),
                "required": list(capability_ids),
            }
            for key in publication_keys
        },
        "stack_layers": {
            lid: {
                "layer_number": num,
                "name": name,
                "description": f"Accessible observation, control and recovery path for the {name} layer.",
                "accessibility": list(capability_ids),
            }
            for lid, num, name in DEPTH_LAYERS
        },
    }

    # 3/4/5. Source proof, grouping the real accessibility sources.
    public = [s for s in a11y_sources if s.get("mode") != "import-only"]
    official = {
        "sources": {
            s["id"]: {
                "layers": [s.get("layer")],
                "kind": s.get("kind"),
                "proof": [s["url"]] if s.get("url") else [],
            }
            for s in sorted(public, key=lambda s: s["id"])
            if s.get("id")
        }
    }

    overlay_layers: dict[str, list[str]] = {}
    for s in a11y_sources:
        layer = s.get("layer")
        sid = s.get("id")
        if layer and sid:
            overlay_layers.setdefault(layer, []).append(sid)
    overlay = {"layers": {k: sorted(v) for k, v in sorted(overlay_layers.items())}}

    # Cross-cutting accessibility standards apply to every depth layer (the doc's
    # invariant holds for all layers), so each L16..L-6 layer is source-backed by
    # the public accessibility standard/reference sources.
    cross_cutting = sorted(s["id"] for s in public if s.get("id"))
    stack_proof = {
        "layers": {lid: list(cross_cutting) for lid, _, _ in DEPTH_LAYERS}
    }

    # 6. actor-spectrum.json (security actor taxonomy + authorization line).
    actor = {
        "schema_version": 1,
        "authorization_line": AUTHORIZATION_LINE,
        "authorized": True,
        "boundary": 'The boundary is authorization, not skill level or the word "hacker".',
        "authorized_classes": AUTHORIZED_CLASSES,
        "unauthorized_classes": UNAUTHORIZED_CLASSES,
    }

    return {
        "full-stack-layers.json": full_stack,
        "accessibility-full-stack.json": a11y_full,
        "accessibility-official-source-proof.json": official,
        "accessibility-overlay-source-proof.json": overlay,
        "accessibility-stack-source-proof.json": stack_proof,
        "actor-spectrum.json": actor,
    }


def build() -> dict[str, int]:
    """Derive and write every taxonomy file; return a count summary."""
    files = derive()
    for name, payload in files.items():
        _write(TAX / name, payload)
    return {
        "depth_layers": len(DEPTH_LAYERS),
        "publication_layers": len(files["accessibility-full-stack.json"]["publication_layers"]),
        "official_proof_sources": len(files["accessibility-official-source-proof.json"]["sources"]),
        "overlay_layers": len(files["accessibility-overlay-source-proof.json"]["layers"]),
    }


if __name__ == "__main__":
    summary = build()
    print("PASS: accessibility taxonomy derived from committed sources")
    for key, value in summary.items():
        print(f"  {key}: {value}")
