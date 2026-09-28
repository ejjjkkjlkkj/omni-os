# omni-os

Dépôt unique pour la partie **UEFI** et **OS** accessible : lecteur d'écran UEFI,
firmware EDK2, noyau x86_64 Rust, navigation BIOS parlée et moteur de voix.

Constitué le 2026-09-28 après audit de tous les dépôts (GitHub `ejjjkkjlkkj/*` et
clones locaux). **Exclus volontairement : NVDA, UTM, Android** (et GNS3, sans lien).

## Contenu de `main`

| Dossier | Source | Version retenue | Contenu |
|---|---|---|---|
| `os/` | `accessible-windows` | `kernel-xhci-hid-enum-20260927` (`f395d44`) | OS Rust : `boot/uefi` (bootloader + lecteur d'écran : hda, hii_ifr, audio, synth, usb_audio, virtio_snd…), `kernel/x86_64` (IPC, handles, ACPI, NVMe, xHCI, USB HID), 30 crates `aw-*` |
| `uefi-screenreader/` | `accessible-windows` | `uefi-screenreader-omni-key-20260927` (`a34721f`) | Lecteur d'écran UEFI / WinRE accessible, HII, navigateur BIOS (NAV.BIN), labs et preuves |
| `solution/` | `solution` | `main` (`5e8947e`) | Firmware EDK2 `OmniPkg` (OmniProbe HDA), outils, tests, **sécurité** (données omni-security intégrées) |
| `navigation/` | `project` | `main` (`1c0852c`) | Navigation UEFI parlée (IFR, ASUS setup), voix v4/v5 |
| `voice-st/` | `st` (local uniquement) | `st-nextgen-quality` (`fcbd2f1`) | Moteur TTS ST (Rust), intégration SAPI5 pour WinPE/WinRE |
| `salvage/` | clones locaux | — | Travail **non commité** récupéré (voir ci-dessous) |

Chaque dossier est **identique octet pour octet** à sa source (même empreinte d'arbre git)
et garde **tout son historique** (fusion de sous-arbre : `git log` sur les commits importés).

## Ce qui a été sauvé et n'existait nulle part ailleurs

- **`st`** : dépôt sans aucune copie GitHub (23 commits) → `voice-st/` + `archive/st/*`.
- **17 branches `solution`** supprimées de GitHub mais non fusionnées (HDA DMA/waveform,
  voicecore, omni-guardian sécurité, pre-cleanup…) → `archive/solution-omni-next-remotes/*`.
  Note : le firmware HDA (`OmniProbe.c`) de la dernière branche HDA est identique à `main`.
- **Modifications non commitées** :
  - `C:\st` : `speak.rs` (+15 lignes) → appliqué dans `voice-st/`.
  - `C:\accessible-windows-gdt-idt-v3` : `hda.rs` (+152/−35), `setup.rs`, `fat16.rs`
    (base `9354495`, pas le tip de `os/`) → `salvage/accessible-windows-gdt-idt-v3/`
    (patch + copies complètes) et `screencore-v1.7`.
  - `C:\aw-kernel` : `run-proofs-system.ps1` + journaux de preuve → `salvage/aw-kernel/`.

## Branches d'archive

Toutes les branches des sources dont le travail n'est pas dans `main` sont conservées
sous `archive/<source>/<branche>` (73 branches). Vérifié : **0 commit source non couvert**
(3 264 commits). Index complet : [ARCHIVE.md](ARCHIVE.md).

Composants UEFI présents **uniquement** dans des branches parallèles (non fusionnables sans risque) :

| Composant | Branche d'archive |
|---|---|
| `boot/uefi-screenreader-core-v1` | `archive/accessible-windows/uefi-screenreader-live-integration-v2-20260922` |
| `boot/uefi-xhci-direct-stage1..3-v1`, `uefi-usb-audio-baseline-v1` | `archive/accessible-windows/uefi-native-voice-v4-20260921` |
| `boot/uefi-hii-graph-prompt-speech-v2` | `archive/accessible-windows/repo-clean-consolidation-20260924` |
| `boot/uefi-native-speech-v2..v4`, `voicecore-v5`, `uefi-accessibility-platform-v1/v2` | `archive/accessible-windows/uefi-realtime-screenreader-20260919` |
| Paquet de publication Microsoft preview | `archive/accessible-windows/release/microsoft-preview-20260924` |
| Intégration NVDA de ST (exclue de `main`) | `archive/st/st-nextgen-quality` |

## Vérification continue (CI `omni-os CI`, à chaque push et PR)

| Job | Ce qui est prouvé |
|---|---|
| `integrity` | provenance des 5 composants, 74 archives intactes, aucun secret, aucun fichier > 50 Mio |
| `os` | rustfmt + clippy `-D warnings` (workspace, UEFI, noyau), tests du workspace, builds UEFI et noyau, lockfiles inchangés |
| `boot` | **démarrage réel dans QEMU** (q35 + OVMF, NVMe/xHCI/HDA) : chargeur UEFI, autotest du lecteur d'écran, passage au noyau, pagination, PCI, ordonnanceur, préemption (si timer), idle ; échec sur toute panique ou exception |
| `voice-st` | rustfmt + clippy `-D warnings`, 39 tests (Windows, cible WinPE/WinRE) |
| `solution` | 278 tests + 37 tests `.omni-agent` |
| `navigation` | 5 contrats de navigation et de voix UEFI |

`integrity daily` revérifie les archives chaque jour.

## Non inclus (volontairement)

- NVDA-\*, UTM-\*, `android`, GNS3, `serveur` (4 fichiers, référence NVDA).
- `omni-security` : 56/58 fichiers déjà dans `solution/` ; ses 2 branches sont en archive.
- Sorties de build et images disque (`target-*`, `artifacts/physical-usb`, `OMNI-*`, `aw-vm`) :
  régénérables, non source.

## Notes

- Les workflows GitHub des sous-dossiers (`*/.github/workflows`) ne s'exécutent pas à
  cet emplacement ; ils sont conservés comme référence.
- Aucun dépôt source n'a été modifié ni supprimé.
- Sécurité (confidentialité, intégrité, disponibilité) : voir [SECURITY.md](SECURITY.md).
