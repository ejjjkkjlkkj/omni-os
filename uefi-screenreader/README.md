# uefi-screenreader

Le lecteur d'écran qui parle **avant le système d'exploitation** : menus du BIOS/UEFI,
réglages HII et récupération Windows (WinRE), au clavier et à la voix.

Chaque étape est un générateur (Python, parfois C#) qui écrit directement une image
UEFI PE32+ ; elle est accompagnée d'une preuve de laboratoire (`*.labproof`) et du
workflow QEMU qui l'a validée (`.github/workflows/`, conservés comme recettes de test :
ils ne s'exécutent pas depuis ce sous-dossier).

## Organisation

| Dossier | Contenu |
|---|---|
| [`boot/uefi-hii-graph-prompt-speech-v1/`](boot/uefi-hii-graph-prompt-speech-v1/RELEASE.md) | **Application finale** `SCREENREADER.EFI` : pilote audio HDA, navigation de tous les menus du BIOS, voix ST pré-enregistrée (`NAV.BIN`) |
| `boot/uefi-hda-*` | audio HDA pas à pas : sonde, topologie, verbes, PCM, routage, parole |
| `boot/uefi-hii-*` | lecture et navigation des formulaires HII : titres, questions, options, valeurs courantes, validation |
| `boot/uefi-screenreader-*`, `boot/uefi-conout-*` | lecture de la console UEFI et lecteur d'écran interactif |
| `boot/winre-accessible-v1` | environnement de récupération Windows accessible |
| `boot/uefi-physical-*`, `boot/uefi-amd5800h-*`, `boot/uefi-asus-*` | preuves sur matériel réel (AMD Ryzen 7 5800H, ASUS M1603QA) |
| `boot/native-*`, `boot/post-firmware-*` | étapes natives après le firmware |
| `system/native-*` | modules du lecteur d'écran natif (entrée, sortie, session, mémoire, ordonnancement…) |
| `lab0` … `lab26` | premiers laboratoires : image EFI brute, PE32+, puis enchaînements |

## Voir aussi

- [`../os/`](../os/) : bootloader et noyau Rust, dont le lecteur d'écran intégré au chargeur.
- [`../voice-st/`](../voice-st/) : moteur de voix ST.
- Les variantes parallèles (`screenreader-core-v1`, étapes `xhci-direct`, `native-speech` v2 à v4…)
  sont dans les branches `archive/accessible-windows/*` : voir [`../docs/PROVENANCE.md`](../docs/PROVENANCE.md).
