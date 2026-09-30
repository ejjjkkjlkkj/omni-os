# omni-os et UEFI 2.11, chapitre par chapitre

Référence : [spécification UEFI 2.11](https://uefi.org/specs/UEFI/2.11/) (décembre 2024, dernière
version publiée). Chaque chapitre est classé ; aucun n'est laissé sans réponse.

- **Utilisé** : omni-os s'en sert, et une preuve de la CI l'exige (marqueur `AW_*` ou test).
- **Détecté** : omni-os interroge le firmware et annonce ce qu'il fournit, à chaque démarrage.
- **Hors rôle** : la norme confie cette partie au firmware ou à un pilote, pas à un chargeur ni à
  un système ; omni-os ne peut pas « l'implémenter » sans remplacer le firmware de la machine.

**Inventaire complet des protocoles.** À chaque démarrage, le chargeur teste les **272 protocoles**
que déclare la référence officielle TianoCore EDK II (`MdePkg/MdePkg.dec`, `edk2-stable202608`,
table générée par [`tools/uefi/gen-protocols.py`](../tools/uefi/gen-protocols.py)) et annonce
chacun de ceux que le firmware installe, avec son nombre d'instances (`AW_UEFI_PROTOCOL`,
`AW_UEFI_INVENTORY known=272 present=N`, exigé par [`check-log.sh`](../tools/boot/check-log.sh)).
Sur le firmware de la CI (OVMF), 128 sont présents.

| Chap. | Titre (UEFI 2.11) | omni-os | Classement | Preuve |
|---|---|---|---|---|
| 1 | Introduction | application UEFI PE32+ x64, sous-système 10 (`EFI_APPLICATION`), NX, W^X, relocations | Utilisé | audit PE de `SCREENREADER.EFI` ; chargeur construit pour `x86_64-unknown-uefi` |
| 2 | Overview | convention d'appel x64, passage au système par `ExitBootServices` | Utilisé | job `boot` : `AW_EXIT_BOOT_SERVICES_*` |
| 3 | Boot Manager | `BootOrder`/`BootNext` parlés et modifiables ; chemin amovible `\EFI\BOOT\BOOTX64.EFI` (récupération externe) ; `PlatformRecovery####`, `OsRecoveryOrder`, `OsRecovery####` lus | Utilisé ; inscription `OsRecovery####` : hors rôle sans clé du propriétaire (variables authentifiées, `dbr`/KEK) | `recovery` (clé USB), `AW_UEFI_PLATFORM_RECOVERY` |
| 4 | EFI System Table | console, tables de configuration : ACPI, SMBIOS, ESRT, Memory Attributes | Utilisé | `boot` : `AW_ACPI_*`, `AW_UEFI_PLATFORM_ESRT` |
| 5 | GPT Disk Layout | image disque GPT publiée ; lecture et écriture GPT dans le noyau | Utilisé | release : image GPT démarrée ; tests du noyau |
| 6 | Block Translation Table | mémoire persistante NVDIMM | Détecté (protocoles de l'inventaire) ; format BTT hors rôle : aucune NVDIMM visée | inventaire |
| 7 | Boot Services | mémoire, images (`LoadImage`/`StartImage`), protocoles, événements, `Stall`, `ExitBootServices` | Utilisé | `boot`, `recovery`, `network` |
| 8 | Runtime Services | variables (lecture, écriture sur action de l'utilisateur, énumération), horloge, `ResetSystem` ; capsules : hors rôle (NIST SP 800-147, c'est au constructeur de signer) | Utilisé | `recovery` (arrêt confirmé), Setup parlé |
| 9 | EFI Loaded Image | image courante, volume de démarrage | Utilisé | `boot` : `AW_KERNEL_FS_OK` |
| 10 | Device Path Protocol | appareil du volume de démarrage ; chemins des images chaînées | Utilisé | `recovery` (exclusion du volume courant) |
| 11 | UEFI Driver Model | omni-os est une application, pas un pilote : il consomme les pilotes du firmware | Hors rôle ; détecté (`DriverBinding`, `ComponentName2`…) | inventaire |
| 12 | Console Support | clavier (`SimpleTextInput`), texte, écran (`GraphicsOutput`) ; pointeurs détectés | Utilisé | `boot`, `navigation-boot` (touches réelles) |
| 13 | Media Access | `SimpleFileSystem`, fichiers, `BlockIo` ; `RamDisk`, `DiskIo`, `PartitionInfo`, NVMe/ATA pass-thru détectés | Utilisé | `recovery` (écriture de l'état A/B, diagnostic) |
| 14 | PCI Bus Support | `PciIo` (OmniProbe : contrôleur HDA) | Utilisé | release : `probe-evidence.txt` |
| 15 | SCSI Driver Models | pilotes du firmware | Détecté | inventaire |
| 16 | iSCSI Boot | démarrage iSCSI | Détecté ; écarté volontairement (surface d'attaque réseau) | inventaire |
| 17 | USB Support | `UsbIo` : afficheur braille HID, audio USB | Utilisé | `recovery` (`braille=` annoncé), tests `aw-braille` |
| 18 | Debugger Support | réservé aux débogueurs | Hors rôle ; détecté | inventaire |
| 19 | Compression Algorithm | décompression interne au firmware | Hors rôle ; détecté (`Decompress`) | inventaire |
| 20 | ACPI Protocols | tables ACPI lues et validées (RSDP, MCFG, MADT) | Utilisé | `boot` : `AW_ACPI_*`, `AW_PCIE_ECAM_*` |
| 21 | String Services | chaînes HII lues par le lecteur d'écran | Utilisé | `navigation-boot` |
| 22 | EFI Byte Code VM | omni-os livre du code x64 natif | Hors rôle ; détecté (`Ebc`) | inventaire |
| 23 | Firmware Update and Reporting | ESRT lue : composants, versions, dernière tentative ; `FirmwareManagement` inventorié | Utilisé (lecture) ; envoi de capsules hors rôle | `AW_UEFI_PLATFORM_ESRT`, `guardian-inventory.txt` |
| 24 | SNP, PXE, BIS, HTTP Boot | `SimpleNetwork` (cartes, lien) ; récupération HTTP vérifiée par empreinte ; PXE écarté (PixieFail) | Utilisé | `network` |
| 25 | Managed Network | sous la pile IP du firmware | Détecté | inventaire |
| 26 | Bluetooth | dépend du pilote du constructeur | Détecté | inventaire |
| 27 | VLAN, EAP, Wi-Fi, Supplicant | dépend du pilote du constructeur | Détecté | inventaire |
| 28 | TCP, IP, IPsec, FTP, TLS | `Ip4Config2` (DHCP sur demande) ; TLS par la pile HTTP du firmware ; IPsec retiré d'EDK II (2019) | Utilisé | `network` |
| 29 | ARP, DHCP, DNS, HTTP, REST | DHCPv4 ; HTTP (récupération) ; DNS résolu par la pile HTTP du firmware pour une URL par nom | Utilisé | `network` : `AW_UEFI_NET_DHCP_OK`, `AW_UEFI_NET_RECOVERY_VERIFIED` |
| 30 | UDP and MTFTP | sous la pile du firmware | Détecté | inventaire |
| 31 | EFI Redfish Service Support | gestion à distance de serveurs (SMBIOS type 42) | Détecté ; hors rôle sur un poste personnel | inventaire |
| 32 | Secure Boot and Driver Signing | état Secure Boot, `PK`/`KEK`/`db`/`dbx` lus et annoncés ; toute image chaînée passe par `LoadImage`, donc par la politique Secure Boot | Utilisé | `boot` : `AW_UEFI_SECURITY` |
| 33 | Human Interface Infrastructure | formulaires et réglages du BIOS lus à voix haute | Utilisé | `navigation-boot`, `SCREENREADER.EFI` (release) |
| 34 | HII Protocols | `HiiDatabase`, `HiiString`, chaînes et paquets | Utilisé | `navigation-boot` |
| 35 | HII Configuration Processing and Browser Protocol | `HiiConfigRouting` : valeurs des réglages | Utilisé | Setup parlé du chargeur |
| 36 | User Identification | identification d'utilisateur par le firmware | Détecté ; hors rôle | inventaire |
| 37 | Secure Technologies | `Tcg2` (IDS : journal rejoué contre les PCR, noyau mesuré dans le PCR 9) ; `Rng` interrogé ; SHA-256 propre (FIPS 180-4) ; `Hash2`/`Pkcs7Verify` détectés | Utilisé | `measured`, `AW_UEFI_PLATFORM_RNG` |
| 38 | Confidential Computing | mesure en environnement d'exécution de confiance (TDX…) | Détecté (`CcMeasurement`) ; hors rôle hors machine virtuelle confidentielle | inventaire |
| 39 | Miscellaneous Protocols | `Timestamp`, `ResetNotification` | Détecté | inventaire |

## Ce que « 100 % » veut dire ici

- **Tout ce que la norme met à la disposition d'un chargeur et d'un système** est utilisé ou,
  quand le matériel le fournit, détecté et annoncé à chaque démarrage.
- **Ce que la norme confie au firmware** (pilotes, décompression, EBC, débogueur, mises à jour par
  capsule) ne peut être fourni que par le firmware de la machine ; omni-os en vérifie la présence.
- **Ce qui exige une clé du propriétaire** (`OsRecovery####`, changement des clés Secure Boot)
  reste une action du propriétaire, jamais une décision automatique.

Titres vérifiés sur [uefi.org/specs/UEFI/2.11](https://uefi.org/specs/UEFI/2.11/).
