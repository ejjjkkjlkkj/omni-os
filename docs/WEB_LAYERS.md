# OMNI Web-Layer Intelligence Model

The terms "surface", "deep" and "dark" describe reachability/indexing, not trust.

## Layers

| ID | Layer | Meaning |
|---|---|---|
| surface_web | Public indexed web | Search engines, vendor advisories, blogs, public repos |
| deep_authenticated | Non-indexed authenticated web | Portals, vendor/customer systems, private feeds used with authorization |
| closed_community | Restricted community | Invite-only forums, research communities, closed CTI exchanges |
| tor_onion | Tor onion services | Services reachable only through Tor onion routing |
| i2p | I2P services | Services reachable through the I2P overlay |
| zeronet | P2P signed Web | ZeroNet decentralized signed sites |\n| ipfs | Content-addressed P2P | IPFS CID/IPNS/DNSLink content |\n| hyphanet | Privacy/censorship-resistant P2P | Hyphanet, formerly the original Freenet |\n| gnunet | Privacy-preserving P2P framework | GNUnet services |\n| namecoin_bit | Decentralized naming | Namecoin .bit namespace |\n| mixnet | Mix-network layer | Metadata-resistant mix-network services |\n| mesh_overlay | Mesh/routed overlay | Alternative routed overlays |\n| other_overlay | Other anonymity/overlay networks | Additional non-standard overlay networks |
| cti_reporting | Secondary CTI reporting | Reports describing activity from any layer |
| local_seclab | Local isolated collection | Evidence/metadata collected in an authorized OMNI SecLab |

Tor documents onion services as services reachable only through the Tor network.
OMNI records Tor/I2P as source provenance; it does not equate overlay use with
maliciousness.

## What may be stored

Security metadata is allowed regardless of whether the source concerns defensive
or offensive tooling:

- actor/campaign/tool/malware names and aliases;
- public hashes and IOCs;
- CVE/CWE/CAPEC/ATT&CK IDs;
- public infrastructure indicators from recognized CTI sources;
- public market/forum names as entities;
- tool/framework capabilities;
- malware family behavior;
- exploit-kit names;
- C2 families;
- ransomware families;
- dates, source URLs, confidence and attribution wording.

## Personal-data boundary

Do not persist:

- leaked passwords or authentication tokens;
- private keys/session cookies;
- raw stolen credential databases;
- private-person addresses, phone numbers or identity-document data;
- private communications or non-public personal files;
- dossiers about private individuals.

If a source mixes security metadata with private-person data, retain the security
metadata and discard the private-person fields.

## Collection architecture

```text
surface feeds -----------------------------+
authenticated feeds (authorized) ----------+
CTI reports -------------------------------+--> NORMALIZER --> OMNI DB
SecLab Tor/I2P/ZeroNet/IPFS/other imports --+
                                             |
                                             +--> source + timestamp + hash
```

GitHub Actions refreshes safe public feeds. Direct collection from authenticated,
Tor, I2P or other restricted layers belongs in an isolated SecLab and is imported
as normalized metadata.
