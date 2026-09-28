#!/usr/bin/env python3
"""Validate OMNI's master coverage map.

This does not claim omniscience. It enforces that known required domains are
mapped, source-backed, cross-linked to Web/overlay and deep-stack taxonomies,
and that unknowns become explicit coverage gaps rather than silent omissions.
"""
from __future__ import annotations
import json
import pathlib
import sys
from collections import Counter

ROOT=pathlib.Path(__file__).resolve().parents[1]

def load(path):
    return json.loads((ROOT/path).read_text(encoding="utf-8"))

master=load("data/taxonomy/master-coverage.json")
sources_doc=load("data/threat-intel/sources.json")
network=load("data/taxonomy/network-layers.json")
full=load("data/taxonomy/full-stack-layers.json")
a11y=load("data/taxonomy/accessibility-full-stack.json")
actor=load("data/taxonomy/actor-spectrum.json")
langs=load("data/taxonomy/language-security-map.json")
gaps=load("data/threat-intel/coverage-gaps.json")

errors=[]

required_axes={
 "assets","trust_boundaries","threats","attack_surface","protections","detections",
 "response","recovery","evidence","reference_state","attestation_or_verification",
 "privacy","accessibility","privilege_model","update_rollback","source_provenance"
}
required_lifecycle={
 "research","design","development","build","test","manufacture","integration",
 "provisioning","deployment","operation","monitoring","update","incident","recovery",
 "repair_rma","decommission","destruction"
}
anchor_domains={
 "governance-risk","human-social-insider","threat-intelligence-attribution",
 "vulnerability-management","red-team-offensive-testing","blue-team-detection",
 "incident-response","digital-forensics","malware-ransomware-botnet",
 "identity-proofing-authentication","authorization-zero-trust",
 "cryptography-classical","cryptography-post-quantum","privacy-metadata-anonymity",
 "memory-safety-concurrency","application-web-api","mobile-platform-apps",
 "cloud-iaas-paas-saas","containers-kubernetes-cloud-native",
 "virtualization-confidential-computing","kernel-microkernel-security-monitor",
 "drivers-devices-dma-iommu","firmware-uefi-boot","network-routing-transport",
 "overlay-dark-deep-p2p-web","telecom-5g-network-equipment","iot-embedded",
 "ot-ics-scada","automotive-vehicle","medical-device","space-avionics-satellite",
 "ai-ml-models-agents","build-ci-cd-toolchain","supply-chain-manufacturing-logistics",
 "root-of-trust-tpm-dice-hsm-secure-element","silicon-microcode-fuses-debug-entropy",
 "physical-side-channel-fault-injection-tamper","accessibility-screen-readers",
 "accessibility-semantics-apis","accessibility-braille-hid-haptics",
 "accessibility-speech-tts-audio","accessibility-preos-kernel-silicon",
 "payments-fintech-cardholder-data","energy-power-grid-utilities",
 "maritime-shipping-ports","robotics-drones-autonomous-systems",
 "critical-infrastructure-cross-sector","unknown-emerging-unclassified"
}

# Sources
source_ids=[s.get("id") for s in sources_doc.get("sources",[]) if s.get("id")]
duplicates=[k for k,v in Counter(source_ids).items() if v>1]
if duplicates:
    errors.append("duplicate source IDs: "+", ".join(sorted(duplicates)))
source_set=set(source_ids)

# Domains
domains=master.get("domains",[])
domain_ids=[d.get("id") for d in domains if d.get("id")]
dupe_domains=[k for k,v in Counter(domain_ids).items() if v>1]
if dupe_domains:
    errors.append("duplicate domain IDs: "+", ".join(sorted(dupe_domains)))
domain_set=set(domain_ids)
if len(domain_set) < 90:
    errors.append(f"master domain count unexpectedly low: {len(domain_set)} < 90")
missing_anchor=sorted(anchor_domains-domain_set)
if missing_anchor:
    errors.append("missing anchor domains: "+", ".join(missing_anchor))

required_groups=set(master.get("required_groups",[]))
actual_groups={d.get("group") for d in domains if d.get("group")}
missing_groups=sorted(required_groups-actual_groups)
if missing_groups:
    errors.append("required groups without domains: "+", ".join(missing_groups))
for d in domains:
    did=d.get("id") or "<missing-id>"
    if not d.get("group"):
        errors.append(f"{did}: missing group")
    elif d["group"] not in required_groups:
        errors.append(f"{did}: group not declared in required_groups: {d['group']}")
    refs=d.get("sources") or []
    if not refs:
        errors.append(f"{did}: no source references")
    for ref in refs:
        if ref not in source_set:
            errors.append(f"{did}: unknown source {ref}")

# Axes and lifecycle
missing_axes=sorted(required_axes-set(master.get("required_axes",[])))
if missing_axes:
    errors.append("missing required axes: "+", ".join(missing_axes))
missing_lifecycle=sorted(required_lifecycle-set(master.get("lifecycle",[])))
if missing_lifecycle:
    errors.append("missing lifecycle phases: "+", ".join(missing_lifecycle))

# Publication / overlay planes must match the canonical network and accessibility maps.
master_planes=set(master.get("source_planes",[]))
network_planes=set(network.get("publication_layers",{}))
a11y_planes=set(a11y.get("publication_layers",{}))
if master_planes != network_planes:
    errors.append("master source_planes != network publication layers")
if master_planes != a11y_planes:
    errors.append("master source_planes != accessibility publication layers")

# Deep stack must be 23 aligned layers L16..L-6.
full_layers={x.get("id") for x in full.get("layers",[]) if x.get("id")}
a11y_layers=set(a11y.get("stack_layers",{}))
if len(full_layers) != 23:
    errors.append(f"deep security stack expected 23 layers, got {len(full_layers)}")
if full_layers != a11y_layers:
    errors.append("security and accessibility deep-stack layer IDs differ")

# Supporting taxonomies must not be empty.
if not actor.get("authorization_line") and not actor.get("authorized"):
    errors.append("actor spectrum taxonomy is empty")
if len(langs.get("languages",{})) < 20:
    errors.append("language security map unexpectedly small")
if not master.get("completeness_invariant"):
    errors.append("missing completeness invariant")

# Coverage gaps must be explicit and structured.
if not isinstance(gaps.get("gaps"),list):
    errors.append("coverage-gaps.json gaps must be an array")
else:
    for gap in gaps["gaps"]:
        for field in ("id","domain","description","status"):
            if not gap.get(field):
                errors.append(f"coverage gap missing {field}: {gap}")

if errors:
    for error in errors:
        print("ERROR:",error)
    print(f"FAIL: {len(errors)} master coverage error(s)")
    sys.exit(1)

counts=Counter(d["group"] for d in domains)
print(f"PASS: {len(domain_set)} master domains")
print(f"PASS: {len(required_groups)} required domain groups")
print(f"PASS: {len(required_axes)} mandatory assurance axes")
print(f"PASS: {len(master_planes)} Surface/Deep/Dark/overlay source planes")
print(f"PASS: {len(full_layers)} security/accessibility layers L16 through L-6")
print(f"PASS: {len(source_set)} configured source IDs")
print("PASS: all master-domain source references resolve")
print("PASS: unknown/emerging technology is an explicit coverage-gap path")
print("GROUPS:",json.dumps(dict(sorted(counts.items())),sort_keys=True))
