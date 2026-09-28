# OMNI Deepest Security Map

This map is intentionally broader than the OSI model. It links human authorization,
software, networks, platform security, silicon and physical supply-chain trust.

## Complete analytical stack

```text
L16  GOVERNANCE / LEGAL / AUTHORIZATION
     mandate, owner consent, scope, policy, jurisdiction

L15  HUMAN / SOCIAL / OPERATOR
     social engineering, coercion, mistakes, insider risk, secure attention

L14  IDENTITY / CREDENTIALS / MACHINE IDENTITY
     accounts, passkeys, certificates, device identities, workload identities

L13  APPLICATION / BUSINESS LOGIC
     browser, API, update client, package manager, security tools

L12  DATA / CONTENT / FORMATS
     documents, media, archives, models, serialized data, untrusted parsers

L11  LANGUAGE / RUNTIME / VM
     C/C++/Rust/Go/Python/PowerShell/JS, JVM/.NET, Wasm, eBPF, JITs

L10  LIBRARIES / PACKAGES / BUILD / CI
     dependencies, registries, compilers, build systems, GitHub Actions, provenance

L9   APP SANDBOX / IPC / SERVICES
     capabilities, namespaces, brokers, service boundaries, entitlement systems

L8   NAMING / DISCOVERY
     DNS/DNSSEC, DoH/DoT, DHTs, IPNS, Namecoin, GNS, service discovery

L7   SESSION / CRYPTO
     TLS, SSH, Noise, WireGuard, E2E encryption, PQC/hybrid crypto

L6   OVERLAY / PRIVACY / P2P
     Tor, I2P, ZeroNet, IPFS/libp2p, Hyphanet, GNUnet, Yggdrasil,
     cjdns/Hyperboria, Nym/mixnets, mesh/other overlays

L5   TRANSPORT
     TCP, UDP, QUIC, SCTP-like transports

L4   NETWORK / ROUTING
     IPv4/IPv6, ICMP, BGP, tunnels, routing policy

L3   LINK / RADIO / LOCAL
     Ethernet, Wi-Fi, Bluetooth, NFC, cellular, USB networking, satellite links

L2   DEVICE / DRIVER / DMA
     NIC/GPU/NVMe/HDA/USB/modem, MMIO, IRQ, DMA, IOMMU/SMMU, device assignment

L1   KERNEL / HYPERVISOR / SECURITY MONITOR
     kernel, microkernel, pKVM-like layer, Arm CCA RMM, Intel TDX module

L0   PLATFORM FIRMWARE / BOOT
     UEFI/PI, bootloader, BMC/EC, option ROM, device firmware, measured/secure boot

L-1  COMPONENT AUTHENTICATION / LINK SECURITY
     SPDM measurements/authentication, PCIe DOE/CMA/IDE/TDISP, component RIM

L-2  ROOT OF TRUST / ATTESTATION
     TPM, DICE/DPE, Secure Element, HSM, Caliptra/OpenTitan, RATS/EAT evidence

L-3  SILICON SECURITY STATE
     Boot ROM, OTP/fuses, lifecycle controller, key ladders, entropy/CSRNG,
     debug/JTAG policy, power/reset/clock security, microcode/security engines

L-4  SEMICONDUCTOR / BOARD / MANUFACTURING
     IP blocks, RTL/netlist, masks, fab, packaging, PCB, component provenance,
     counterfeit/tamper risk, supplier and manufacturing test state

L-5  PHYSICAL / ENVIRONMENTAL
     invasive access, fault injection, voltage/clock glitching, EM/side-channel,
     bus interposers, malicious peripherals, physical replacement/tampering

L-6  SUPPLY-CHAIN / LIFECYCLE
     design -> source -> build -> manufacture -> integration -> transport ->
     provisioning -> operation -> update -> repair/RMA -> decommission
```

## Why this goes below "hardware root"

A hardware root of trust is itself implemented in silicon and has a manufacturing
history, lifecycle state, debug policy, entropy source and provisioning process.
Therefore "trust hardware" is not the end of the analysis.

NIST platform guidance treats the physical platform as the foundation of layered
security and explicitly considers component provenance/tamper throughout the
device lifecycle. TCG RIM/DICE and DMTF SPDM make measurement and reference
comparison machine-verifiable.

## Protect / Detect / Recover

For each layer OMNI should record:

- assets;
- trust assumptions;
- threats;
- non-bypassable invariants;
- protection mechanism;
- detection mechanism;
- recovery mechanism;
- evidence;
- reference state;
- attestation path;
- update/rollback path;
- accessibility impact.

## Component security

A future OMNI device inventory should distinguish:

```text
PRESENT != AUTHENTICATED
AUTHENTICATED != MEASURED
MEASURED != MATCHES_REFERENCE
MATCHES_REFERENCE != AUTHORIZED
AUTHORIZED != HEALTHY_FOREVER
```

Target chain:

```text
device identity
  -> SPDM/DICE evidence
  -> RIM/reference values
  -> verifier policy
  -> authorization decision
  -> continuous re-evaluation where practical
```

For PCIe-capable future hardware, link protection and device assignment should
consider SPDM plus PCIe IDE/TDISP-class mechanisms rather than trusting bus
enumeration alone.

## Silicon-root model

The strongest long-term OMNI model is:

```text
manufacturing provenance
        ↓
immutable/minimal Boot ROM
        ↓
silicon lifecycle + fused/OTP identity
        ↓
DICE/DPE / RoT key ladder
        ↓
measured firmware
        ↓
security monitor / hypervisor
        ↓
OMNI Kernel
        ↓
capability-scoped userspace
```

Root secrets should be used through derived/sideloaded keys where possible rather
than being readable by ordinary software.

## Unknown-coverage invariant

Any technology that cannot be mapped to a layer, source, threat model and
verification strategy creates a **coverage gap**. "Unknown" is never interpreted
as "safe".
