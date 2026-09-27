# OMNI Network / Web / Root Map

OMNI models two orthogonal dimensions:

1. **Where information/services are reachable** — Surface Web, Deep Web, Tor, I2P, ZeroNet, IPFS, Hyphanet, GNUnet and other overlays.
2. **Where enforcement happens in the stack** — application down to transport, routing, link, firmware, hardware and roots of trust.

A source can therefore be, for example, `tor_onion` at the publication layer while the observed technique affects DNS, TLS, a browser, a kernel driver, or firmware.

## Publication and overlay layers

| ID | Type | Examples / meaning |
|---|---|---|
| surface_web | Indexed public Web | HTTP(S), public repos, advisories, vendor sites |
| deep_unindexed | Public but not indexed | Generated pages, obscure endpoints, non-indexed services |
| deep_authenticated | Authenticated Web | Portals, customer/vendor systems, private feeds used with authorization |
| closed_community | Restricted community | Invite-only research/CTI communities |
| tor_onion | Anonymity overlay | Tor onion services |
| i2p | Anonymity overlay | I2P services / eepsites |
| zeronet | P2P signed Web | ZeroNet sites distributed peer-to-peer |
| ipfs | Content-addressed P2P | IPFS CIDs/IPNS/DNSLink and gateways |
| hyphanet | Privacy/censorship-resistant P2P | Hyphanet, formerly original Freenet |
| gnunet | Privacy-preserving P2P framework | GNUnet services |
| namecoin_bit | Decentralized naming | Namecoin .bit naming |
| p2p_overlay | Generic P2P/overlay | BitTorrent-like or other decentralized publication |
| mixnet | Mix-network layer | Systems whose primary design is metadata resistance through mixing |
| mesh_overlay | Mesh/routed overlay | Overlay routing independent of ordinary DNS/Web naming |
| other_overlay | Extensible unknown overlay | New or uncommon systems not yet assigned a dedicated class |
| cti_reporting | Secondary reporting | Reports about any of the layers above |
| local_seclab | Authorized local import | OMNI SecLab normalized observations |

ZeroNet is treated as a distinct source/protocol family because its official project describes peer-distributed sites, signed content metadata and Tor integration.

IPFS is treated separately because content is addressed by cryptographic CIDs rather than ordinary location-only URLs.

Hyphanet is recorded as the continuation of the original Freenet project.

## Network and trust stack

```text
L12  HUMAN / AUTHORIZATION
     user intent, operator approval, social engineering

L11  APPLICATION / CONTENT
     browser, API, email, chat, package manager, CTI feed

L10  APPLICATION SECURITY
     sandbox, capabilities, authn/authz, parser boundaries

L9   NAMING / DISCOVERY
     DNS, DNSSEC, DoH/DoT, Namecoin, IPNS, DHTs, service discovery

L8   CRYPTO / SESSION
     TLS, SSH, Noise, WireGuard, application E2E crypto

L7   OVERLAY / PRIVACY
     Tor, I2P, ZeroNet, IPFS/libp2p, Hyphanet, GNUnet, mixnets

L6   TRANSPORT
     TCP, UDP, QUIC

L5   NETWORK / ROUTING
     IPv4, IPv6, ICMP, BGP, routing policy, tunnels

L4   LINK / LOCAL NETWORK
     Ethernet, Wi-Fi, Bluetooth, NFC, cellular link, ARP/NDP

L3   DEVICE / DRIVER
     NIC, modem, USB network device, driver, DMA mappings, IOMMU

L2   KERNEL / HYPERVISOR
     socket mediation, firewall enforcement, namespaces, pKVM/hypervisor

L1   FIRMWARE / BOOT
     UEFI, device firmware, option ROM, secure/measured boot

L0   HARDWARE ROOT
     Boot ROM, TPM, Secure Element, hardware RoT, fused keys
```

The numbering is an OMNI analytical model, not the OSI model.

## Root principle

Security claims must state the lowest layer they trust.

Examples:

- a browser sandbox claim may trust kernel/hypervisor below it;
- a kernel-integrity claim may trust a security monitor and hardware RoT;
- an attestation claim may trust TPM/Secure Element keys and measured boot;
- a dark-web observation does not become more trustworthy simply because it came from Tor/I2P.

## Coverage invariant

Every new protocol, overlay, language, runtime, device type or trust primitive that does not map cleanly to this taxonomy must create a **coverage-gap record**. Unknown does not mean safe.
