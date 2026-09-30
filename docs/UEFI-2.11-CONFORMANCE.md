# omni-os et UEFI 2.11, chapitre par chapitre

Référence : [spécification UEFI 2.11](https://uefi.org/specs/UEFI/2.11/) (décembre 2024, dernière
version publiée). Chaque chapitre est classé ; aucun n'est laissé sans réponse.

- **Utilisé** : omni-os appelle le protocole et vérifie la réponse, et une preuve de la CI l'exige
  (marqueur `AW_*` ou test).
- **Absent d'OVMF** : le firmware de la CI ne fournit pas ce protocole ; quand il a un sens pour un
  chargeur, le code d'appel existe et sert dès qu'une machine le fournit.
- **Hors rôle** : la norme confie cette partie au firmware ou à un pilote, pas à un chargeur ni à
  un système ; omni-os ne peut pas « l'implémenter » sans remplacer le firmware de la machine.

**Tous les protocoles présents sont utilisés.** À chaque démarrage, le chargeur cherche les
**272 protocoles** que déclare la référence officielle TianoCore EDK II (`MdePkg/MdePkg.dec`,
`edk2-stable202608`, table générée par [`tools/uefi/gen-protocols.py`](../tools/uefi/gen-protocols.py)).
Pour chaque protocole installé par le firmware, [`protocols.rs`](../os/boot/uefi/src/protocols.rs)
vérifie que chaque interface est en mémoire du firmware, puis **appelle le protocole et contrôle la
réponse**. Une réponse fausse, ou un protocole présent sans usage, fait échouer la preuve de
démarrage ([`check-log.sh`](../tools/boot/check-log.sh) exige `failed=0 unclassified=0`).

**Les 272 ont chacun un usage écrit dans le code**, même ceux qu'OVMF ne fournit pas (SMM,
Wi-Fi, UFS, SD/MMC, PKCS7, TCG 1.2, NVDIMM…) : sur une machine qui les installe, ils sont
appelés à leur tour (par exemple le verrou SMRAM lu par `SmmAccess2`, ou une signature
malformée qui doit être refusée par `Pkcs7Verify`). La CI le vérifie
([`check-protocol-coverage.py`](../tools/uefi/check-protocol-coverage.py)).

Résultat mesuré sur le firmware de la CI (OVMF, QEMU q35) :
`AW_UEFI_PROTOCOLS present=128 exercised=119 marker=5 guarded=4 failed=0 unclassified=0`.

Exemples d'appels réels, chacun vérifié :

- secteur 0 lu par `BlockIo`, `BlockIo2`, `DiskIo` et `DiskIo2`, octets identiques ;
- SHA-256 calculé par `Hash2`, identique au SHA-256 d'omni-os (FIPS 180-4) ;
- un enfant créé puis détruit sur chacun des 16 services réseau (`Arp`, `Dhcp4/6`, `Dns4/6`,
  `Http`, `Ip4/6`, `ManagedNetwork`, `Mtftp4/6`, `Tcp4/6`, `Tls`, `Udp4/6`, `Hash2`), sans rien émettre ;
- une table ACPI SSDT installée, relue par `AcpiSdt`, puis retirée ; sommes de contrôle vérifiées ;
- un disque RAM de 64 Kio enregistré, relu par `BlockIo`, puis retiré ;
- un code d'état émis par `StatusCodeRuntime` et reçu par notre écouteur `RscHandler` ;
- l'image d'omni-os soumise à la politique Secure Boot du firmware (`Security2Arch`) ;
- 65 pilotes nommés (`ComponentName2`), 140 chemins d'appareils convertis en texte, 118 fichiers
  du volume firmware énumérés, en-têtes `_FVH` vérifiés, SMBIOS types 0 et 1 lus.

**Marqueurs (5)** : protocoles que la norme PI définit sans interface (`CapsuleArch`, `ResetArch`,
`PciEnumerationComplete`, `DxeMmReadyToLock`, `DxeSmmReadyToLock`). Leur installation est tout le
contrat. Les services qu'ils annoncent (horloge, compteur, variables) sont vérifiés sur leur
propre ligne.

**Protégés (4), jamais appelés ; la raison est écrite dans le marqueur :**

| Protocole | Pourquoi il n'est pas appelé |
|---|---|
| `BdsArch` | `Entry` lance le gestionnaire de démarrage, c'est-à-dire quitte omni-os |
| `LoadFile` (cartes réseau) | tout appel déclenche un démarrage PXE ou HTTP, donc du trafic ; le réseau reste fermé par défaut. `LoadFile2` et les `LoadFile` hors réseau sont appelés |
| `S3SaveState` | écrit le script de reprise après veille du firmware |
| `PciHotPlugRequest` | ajoute ou retire des périphériques PCI |

| Chap. | Titre (UEFI 2.11) | omni-os | Classement | Preuve |
|---|---|---|---|---|
| 1 | Introduction | application UEFI PE32+ x64, sous-système 10 (`EFI_APPLICATION`), NX, W^X, relocations | Utilisé | audit PE de `SCREENREADER.EFI` ; chargeur construit pour `x86_64-unknown-uefi` |
| 2 | Overview | convention d'appel x64, passage au système par `ExitBootServices` | Utilisé | job `boot` : `AW_EXIT_BOOT_SERVICES_*` |
| 3 | Boot Manager | `BootOrder`/`BootNext` parlés et modifiables ; chemin amovible `\EFI\BOOT\BOOTX64.EFI` (récupération externe) ; `PlatformRecovery####`, `OsRecoveryOrder`, `OsRecovery####` lus | Utilisé ; inscription `OsRecovery####` : hors rôle sans clé du propriétaire (variables authentifiées, `dbr`/KEK) | `recovery` (clé USB), `AW_UEFI_PLATFORM_RECOVERY` |
| 4 | EFI System Table | console, tables de configuration : ACPI, SMBIOS, ESRT ; Memory Attributes Table lue pour donner au noyau le code runtime en lecture seule et exécutable (W^X) | Utilisé | `boot` : `AW_ACPI_*`, `AW_UEFI_PLATFORM_ESRT`, `AW_UEFI_RUNTIME_HANDOFF` |
| 5 | GPT Disk Layout | image disque GPT publiée ; lecture et écriture GPT dans le noyau | Utilisé | release : image GPT démarrée ; tests du noyau |
| 6 | Block Translation Table | mémoire persistante NVDIMM | Absent d'OVMF ; format BTT hors rôle : aucune NVDIMM visée | inventaire |
| 7 | Boot Services | mémoire, images (`LoadImage`/`StartImage`), protocoles, événements, `Stall`, `ConnectController` récursif (clé de récupération branchée après le démarrage), `ExitBootServices` | Utilisé | `boot`, `recovery`, `network` |
| 8 | Runtime Services | chargeur : variables (lecture, écriture sur action de l'utilisateur, énumération), horloge, `ResetSystem`. **Noyau, après `ExitBootServices`** : `SetVariable`/`GetVariable` pour le bilan de santé d'une génération à l'essai, soit par le code runtime projeté en lecture-exécution (Memory Attributes Table), soit sur les tables de pages du firmware le temps de l'appel ; les tables du noyau restent W^X. Capsules : hors rôle (NIST SP 800-147, c'est au constructeur de signer) | Utilisé | `recovery` (promotion, arrêt confirmé), `boot` : `AW_UEFI_RUNTIME_READY`, Setup parlé |
| 9 | EFI Loaded Image | image courante, volume de démarrage | Utilisé | `boot` : `AW_KERNEL_FS_OK` |
| 10 | Device Path Protocol | appareil du volume de démarrage ; chemins des images chaînées | Utilisé | `recovery` (exclusion du volume courant) |
| 11 | UEFI Driver Model | `DriverBinding` (64 pilotes, image de chacun vérifiée), `ComponentName`/`ComponentName2` (pilotes nommés), `BusSpecificDriverOverride`, `DriverSupportedEfiVersion` | Utilisé (omni-os consomme les pilotes du firmware) | `AW_UEFI_PROTOCOL` |
| 12 | Console Support | clavier (`SimpleTextInput`), texte, écran (`GraphicsOutput`) ; pointeurs détectés | Utilisé | `boot`, `navigation-boot` (touches réelles) |
| 13 | Media Access | `SimpleFileSystem`, `BlockIo`, `BlockIo2`, `DiskIo`, `DiskIo2` (lectures comparées), `DiskInfo` (Identify), `RamDisk` (enregistré, relu, retiré), `LoadFile2`, `StorageSecurityCommand`, NVMe et ATA pass-thru | Utilisé | `recovery`, `AW_UEFI_PROTOCOL` |
| 14 | PCI Bus Support | `PciIo` (emplacement et espace de configuration de chaque fonction), `PciRootBridgeIo` (fenêtres de ressources) ; OmniProbe : contrôleur HDA | Utilisé | `AW_UEFI_PROTOCOL`, `probe-evidence.txt` |
| 15 | SCSI Driver Models | `ExtScsiPassThru` : cibles énumérées | Utilisé | `AW_UEFI_PROTOCOL` |
| 16 | iSCSI Boot | `IScsiInitiatorName` lu ; aucun démarrage iSCSI (surface d'attaque réseau) | Utilisé (lecture) | `AW_UEFI_PROTOCOL` |
| 17 | USB Support | `Usb2Hc` (capacités, ports racine), `UsbIo` : afficheur braille HID, audio USB | Utilisé | `recovery` (`braille=` annoncé), tests `aw-braille` |
| 18 | Debugger Support | `DebugSupport` : architecture et processeurs lus ; aucun rappel installé | Utilisé (lecture) | `AW_UEFI_PROTOCOL` |
| 19 | Compression Algorithm | `Decompress.GetInfo` sur un en-tête construit | Utilisé | `AW_UEFI_PROTOCOL` |
| 20 | ACPI Protocols | tables ACPI lues et validées (RSDP, MCFG, MADT) | Utilisé | `boot` : `AW_ACPI_*`, `AW_PCIE_ECAM_*` |
| 21 | String Services | chaînes HII lues par le lecteur d'écran | Utilisé | `navigation-boot` |
| 22 | EFI Byte Code VM | `Ebc.GetVersion` ; omni-os livre du code x64 natif | Utilisé (lecture) | `AW_UEFI_PROTOCOL` |
| 23 | Firmware Update and Reporting | ESRT lue : composants, versions, dernière tentative ; `FirmwareManagement` inventorié | Utilisé (lecture) ; envoi de capsules hors rôle | `AW_UEFI_PLATFORM_ESRT`, `guardian-inventory.txt` |
| 24 | SNP, PXE, BIS, HTTP Boot | `SimpleNetwork` (cartes, lien) ; récupération HTTP vérifiée par empreinte ; PXE écarté (PixieFail) | Utilisé | `network` |
| 25 | Managed Network | instance créée et détruite par `ManagedNetworkServiceBinding` | Utilisé | `AW_UEFI_PROTOCOL` |
| 26 | Bluetooth | dépend du pilote du constructeur | Absent d'OVMF | inventaire |
| 27 | VLAN, EAP, Wi-Fi, Supplicant | `VlanConfig.Find` ; Wi-Fi et EAP dépendent du pilote du constructeur | Utilisé (VLAN) ; Wi-Fi et EAP absents d'OVMF | `AW_UEFI_PROTOCOL` |
| 28 | TCP, IP, IPsec, FTP, TLS | `Ip4Config2`/`Ip6Config` lus ; instances `Tcp4/6`, `Ip4/6`, `Tls` créées et détruites ; DHCP sur demande ; IPsec retiré d'EDK II (2019) | Utilisé | `network`, `AW_UEFI_PROTOCOL` |
| 29 | ARP, DHCP, DNS, HTTP, REST | instances `Arp`, `Dhcp4/6`, `Dns4/6`, `Http` créées et détruites ; `HttpUtilities.Parse` ; récupération HTTP | Utilisé | `network` : `AW_UEFI_NET_RECOVERY_VERIFIED`, `AW_UEFI_PROTOCOL` |
| 30 | UDP and MTFTP | instances `Udp4/6`, `Mtftp4/6` créées et détruites | Utilisé | `AW_UEFI_PROTOCOL` |
| 31 | EFI Redfish Service Support | gestion à distance de serveurs (SMBIOS type 42) | Absent d'OVMF ; hors rôle sur un poste personnel | inventaire |
| 32 | Secure Boot and Driver Signing | état Secure Boot, `PK`/`KEK`/`db`/`dbx` lus et annoncés ; toute image chaînée passe par `LoadImage`, donc par la politique Secure Boot | Utilisé | `boot` : `AW_UEFI_SECURITY` |
| 33 | Human Interface Infrastructure | formulaires et réglages du BIOS lus à voix haute ; `FormBrowser2` interrogé | Utilisé | `navigation-boot`, `SCREENREADER.EFI` (release) |
| 34 | HII Protocols | `HiiDatabase`, `HiiString`, `HiiFont` (glyphe rendu), `HiiImage`/`HiiImageEx`, `HiiPackageList`, `HiiPopup` | Utilisé | `navigation-boot`, `AW_UEFI_PROTOCOL` |
| 35 | HII Configuration Processing and Browser Protocol | `HiiConfigRouting.ExportConfig`, `HiiConfigAccess.ExtractConfig` de chaque pilote, `ConfigKeywordHandler` | Utilisé | Setup parlé, `AW_UEFI_PROTOCOL` |
| 36 | User Identification | identification d'utilisateur par le firmware | Absent d'OVMF ; hors rôle | inventaire |
| 37 | Secure Technologies | `Tcg2` (IDS : journal rejoué contre les PCR, noyau mesuré dans le PCR 9, capacités lues) ; `Rng` (algorithmes, entropie) ; `Hash2` comparé à notre SHA-256 ; `Pkcs7Verify` absent d'OVMF | Utilisé | `measured`, `AW_UEFI_PROTOCOL` |
| 38 | Confidential Computing | mesure en environnement d'exécution de confiance (TDX…) | Absent d'OVMF ; hors rôle hors machine virtuelle confidentielle | inventaire |
| 39 | Miscellaneous Protocols | `ResetNotification` (inscription puis retrait) ; `Timestamp` appelé s'il est présent | Utilisé | `AW_UEFI_PROTOCOL` |

## Ce que « 100 % » veut dire ici

- **Tout protocole que le firmware installe est appelé et sa réponse vérifiée**, sauf les 4
  protégés ci-dessus, dont la raison est écrite.
- **Ce que la norme confie au firmware** (pilotes, décompression, EBC, débogueur, mises à jour par
  capsule) ne peut être fourni que par le firmware de la machine ; omni-os en vérifie la présence.
- **Ce qui exige une clé du propriétaire** (`OsRecovery####`, changement des clés Secure Boot)
  reste une action du propriétaire, jamais une décision automatique.

Titres vérifiés sur [uefi.org/specs/UEFI/2.11](https://uefi.org/specs/UEFI/2.11/).
