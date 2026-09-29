# Tout ce qu'un UEFI peut faire, et où en est omni-os

Objectif : un environnement avant système **aussi complet que possible**, et entièrement
accessible. Ce document recense ce que permettent la spécification UEFI 2.11 et ses
implémentations (EDK II), le compare à ce qu'omni-os fait réellement aujourd'hui (vérifié
dans le code et par la CI), et fixe l'ordre de ce qui reste à faire.

Deux sources de capacités coexistent :

- **le firmware de la machine**, qui *peut* fournir des protocoles standard (réseau, TLS,
  stockage…) mais sans garantie : chaque constructeur choisit ce qu'il intègre ;
- **omni-os lui-même**, qui apporte ses propres pilotes quand le standard n'existe pas ou
  n'est pas fiable (l'audio, par exemple, n'a aucun protocole UEFI standard).

Légende : **fait** (prouvé en CI ou sur matériel), **partiel**, **à faire**, **écarté** (avec la raison).

## 1. Accessibilité et interaction

| Capacité | Standard UEFI / EDK II | omni-os | État |
|---|---|---|---|
| Lecture des menus et réglages du BIOS | HII (formulaires IFR, chaînes, `FormBrowser2`) | lecteur d'écran HII (`uefi-screenreader`, `navigation`, `os/boot/uefi/src/hii_ifr.rs`) | fait |
| Synthèse vocale | aucun standard (projet GSoC 2021 non abouti) | synthétiseur formantique FR/EN dans le chargeur ; tous les clips pré-enregistrés (chargeur, noyau, `NAV.BIN`) rendus par la voix ST d'omni-os (`tools/voice/gen-firmware-speech.py`) ; aucune voix tierce | fait |
| Audio | aucun protocole standard | pilotes propres HDA, AC'97, virtio-snd, haut-parleur PC ; USB Audio jusqu'à la configuration du flux | fait (USB Audio : partiel) |
| Braille | aucun standard | afficheur braille HID USB (`os/boot/uefi/src/usb.rs`) | fait |
| Clavier | `SimpleTextInputEx` | clavier du firmware ; PS/2 et USB HID dans le noyau | fait |
| Clavier/souris Bluetooth | `EFI_BLUETOOTH_*` (HC, IO, LE Config), pilotes HID | — | à faire (rarement fourni par les firmwares) |
| Écran | `GraphicsOutput` (GOP) | GOP dans le chargeur, framebuffer et console texte dans le noyau | fait |
| Pointeur tactile / absolu | `AbsolutePointer` | — | à faire |

## 2. Stockage et fichiers

| Capacité | Standard | omni-os | État |
|---|---|---|---|
| Lecture de fichiers (ESP) | `SimpleFileSystem`, FAT | chargement de `KERNEL.BIN` | fait |
| Disques NVMe, SATA, virtio | `NvmExpressPassThru`, `AtaPassThru`, `BlockIo` | pilotes NVMe, AHCI, virtio-blk dans le noyau | fait (noyau) |
| Partitionnement, formatage | `PartitionInfo`, pilotes FAT | GPT + FAT16 écrits par l'installeur du noyau (disques de test uniquement) | partiel |
| Disque en RAM, ISO | `RamDisk` (UEFI 2.5) | — | à faire (utile pour la récupération) |
| Chiffrement de disque, Opal | `StorageSecurityCommand` | — | à faire |

## 3. Réseau

| Capacité | Standard | omni-os | État |
|---|---|---|---|
| Carte réseau brute | `SimpleNetwork` (SNP), UNDI | chargeur : cartes découvertes en lecture seule, état du lien annoncé (commande « réseau »), aucun paquet émis (`AW_UEFI_NET`, `os/boot/uefi/src/net.rs`) ; noyau : pilote virtio-net | partiel : découverte faite, pas encore de trafic |
| IPv4/IPv6, UDP, TCP | `Ip4`/`Ip6`, `Udp4/6`, `Tcp4/6` (EDK II `NetworkPkg`) | IPv4 de la pile du firmware, ouverte seulement sur demande | partiel : IPv4 |
| DHCP, DNS | `Dhcp4/6`, `Dns4/6` | DHCP sur demande explicite (`\OMNI\NET.REQ` à usage unique, ou commande « dhcp » de l'agent) : adresse, masque, passerelle, DNS annoncés (`AW_UEFI_NET_DHCP_OK`) | fait (DHCPv4) ; résolution DNS à faire |
| HTTP, HTTPS (TLS) | `Http`, `Tls`, `TlsConfiguration` ; démarrage HTTP(S) (UEFI 2.5) | — | à faire |
| Wi-Fi | `WirelessMacConnectionII`, `Supplicant`, `EapConfiguration` ; EDK II `WifiConnectionManagerDxe` : WPA2, WPA3 Personal/Enterprise, EAP-TLS/TTLS/PEAP | — | à faire (dépend du pilote Wi-Fi du constructeur) |
| iSCSI, PXE | `IScsiInitiatorName`, `PxeBaseCode` | — | écarté pour l'instant (PXE : surface d'attaque, cf. PixieFail) |
| Redfish, gestion à distance | `RestEx`, Redfish Host Interface (SMBIOS type 42) | — | à faire (serveurs seulement) |

## 4. Sécurité et intégrité

| Capacité | Standard | omni-os | État |
|---|---|---|---|
| Secure Boot | variables PK/KEK/db/dbx authentifiées | état lu et annoncé (`AW_UEFI_SECURITY`) ; gestion des clés par l'agent | fait (lecture) |
| TPM, démarrage mesuré | `Tcg2` : PCR, journal d'événements | présence du TPM et banques PCR annoncées | partiel : journal d'événements non vérifié |
| Hachage, signatures | `Hash2`, `Pkcs7Verify` | — | à faire |
| Aléa matériel | `Rng` | — | à faire |
| Mise à jour du firmware | capsules, `FirmwareManagement`, ESRT | — | à faire |
| Protection mémoire | `MemoryAttribute`, NX | NX, W^X, pages de garde dans le noyau | fait (noyau) |

## 5. Réseau et sécurité avant le démarrage : IPS, IDS, VPN

Ce que la recherche établit :

- **Il n'existe pas de pare-feu, d'IDS ou de VPN standard dans l'UEFI.**
- **L'IPsec d'EDK II (`IpSecDxe`, IKE) a été retiré en 2019** (edk2-stable201905) : peu
  utilisé, et jugé risqué. Un VPN ne peut donc pas s'appuyer sur le firmware.
- Les failles **PixieFail** (2024, pile DHCPv6/PXE d'EDK II) montrent que la pile réseau du
  firmware est elle-même une surface d'attaque.

Ce qu'omni-os fera, dans cet ordre :

1. **IPS par conception (refus par défaut)**. Le chargeur n'ouvre le réseau que sur demande
   explicite (remédiation, règle 9 de
   [`RECOVERY-BOOT-ARCHITECTURE.md`](../os/docs/RECOVERY-BOOT-ARCHITECTURE.md)), et seulement
   vers une liste blanche : DHCP, DNS, et HTTPS vers le serveur de remédiation. Tout le reste
   est ignoré. Les analyseurs de paquets sont minimaux et testés par fuzzing.
2. **IDS d'intégrité du démarrage**. Relire le journal TCG du TPM, recalculer les PCR,
   comparer à une référence connue, et annoncer tout écart à voix haute. S'y ajoutent la
   révocation dbx et l'état Secure Boot, déjà lus.
3. **IDS réseau passif**. Pendant l'utilisation du réseau, détecter les anomalies classiques
   du réseau local : plusieurs serveurs DHCP qui répondent, adresse MAC de la passerelle qui
   change (usurpation ARP), trafic entrant non sollicité. Alerte parlée et journal de preuves.
4. **Transport authentifié avant tout VPN**. La remédiation passe par TLS 1.3 avec clé du
   serveur épinglée et manifeste signé vérifié localement ; cela couvre le besoin sans tunnel.
5. **VPN (type WireGuard)**. Faisable en Rust `no_std` (les primitives Curve25519 et
   ChaCha20-Poly1305 existent sans bibliothèque standard), mais aucune implémentation UEFI
   n'existe : c'est le dernier palier, après les quatre précédents.

## 6. Récupération et gestion

| Capacité | Standard | omni-os | État |
|---|---|---|---|
| WinRE accessible | chargement d'image (`LoadImage`/`StartImage`) | chaîné par le lecteur d'écran (`uefi-screenreader`) ; voix ST dans WinPE/WinRE par SAPI5 | fait |
| Recovery Core natif | — | dans le chargeur : état redondant A/B, noyau vérifié par SHA-256, essai borné et retour automatique, menu parlé au clavier, diagnostic exporté ([`recovery.rs`](../os/boot/uefi/src/recovery.rs)) | fait ; réinstallation signée à faire |
| Démarrage réseau de secours | HTTP(S) Boot + RAM disk | — | à faire, après le réseau |
| Menu de démarrage, BootNext, BootOrder | variables `Boot####` | parlé et modifiable | fait |
| Informations système | SMBIOS, ACPI | lues et annoncées | fait |
| Horloge, variables | `GetTime`/`SetTime`, `SetVariable` | disponibles via l'agent | fait |
| Journal série | `SerialIo` | miroir COM1 des marqueurs | fait |

## Ordre de réalisation

1. **Réseau dans le chargeur, refus par défaut** : découverte des cartes (SNP) et DHCP sur
   demande explicite **faits** et prouvés en CI (`network`).
2. **IDS d'intégrité** : vérification du journal TCG et des PCR, alerte parlée.
3. **HTTPS + manifeste signé** pour la remédiation, puis démarrage réseau de secours.
4. **IDS réseau passif** (DHCP multiples, usurpation ARP).
5. **Recovery Core natif** : fait (état, vérification, essai borné, menu parlé) et prouvé en
   CI (`recovery`) ; restent la réinstallation signée et la validation par le noyau.
6. **Wi-Fi, Bluetooth HID, RAM disk, capsules**, selon le matériel visé.
7. **Tunnel de type WireGuard.**

## Sources

- [Spécification UEFI 2.11](https://uefi.org/specs/UEFI/2.11/index.html) :
  [réseau TCP, IP, IPsec, TLS](https://uefi.org/specs/UEFI/2.11/28_Network_Protocols_TCP_IP_and_Configuration.html),
  [Bluetooth](https://uefi.org/specs/UEFI/2.11/26_Network_Protocols_Bluetooth.html),
  [SNP, PXE, HTTP Boot](https://uefi.org/specs/UEFI/2.10/24_Network_Protocols_SNP_PXE_BIS.html)
- [Retrait d'IpSecDxe, edk2-stable201905](https://github.com/tianocore/edk2/releases/tag/edk2-stable201905)
- [WifiConnectionManagerDxe (EDK II)](https://github.com/tianocore/edk2/blob/master/NetworkPkg/WifiConnectionManagerDxe/WifiConnectionManagerDxe.inf),
  [support WPA3](https://www.mail-archive.com/devel@edk2.groups.io/msg46651.html)
- [HTTP Boot et Redfish (UEFI Plugfest)](https://uefi.org/sites/default/files/resources/UEFI_Plugfest_May_2015_HTTP_Boot_Redfish_Samer_El-Haj_ver1.2.pdf),
  [capsules UEFI (fwupd)](https://github.com/fwupd/fwupd/blob/main/plugins/uefi-capsule/README.md)
- [Démarrage mesuré et TPM (Microsoft)](https://learn.microsoft.com/en-us/windows/security/operating-system-security/system-security/secure-the-windows-10-boot-process),
  [NSA : gestion de Secure Boot](https://media.defense.gov/2025/Dec/11/2003841096/-1/-1/0/CSI_UEFI_SECURE_BOOT.PDF)
- Accessibilité : [Machado, *UEFI BIOS Accessibility for the Visually Impaired*](https://arxiv.org/pdf/1712.03186),
  [rapport GSoC sur un protocole audio EFI](https://gist.github.com/ethindp/82420c25f3c63b6652e4a22766bec95d),
  [Tait Hoyem, UEFI Audio Protocol](https://tait.tech/blog/uefi-audio/)
