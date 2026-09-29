# Lecteur d'écran UEFI — release

Application UEFI (`SCREENREADER.EFI`) qui lit à voix haute et rend navigables au clavier
**tous les menus du BIOS**, avant tout système d'exploitation, avec la voix ST d'omni-os.

## Composants

| Fichier | Rôle | Nature |
|---|---|---|
| `SCREENREADER.EFI` | application UEFI x64 : pilote audio HDA, navigation, lecture | **code** (à signer) |
| `\EFI\OMNI\NAV.BIN` | arbre des menus + toutes les phrases pré-enregistrées | **données**, propres à un modèle de BIOS |
| `OMNI-SR-TRACE.TXT` | journal écrit sur la clé à chaque démarrage | diagnostic |

`NAV.BIN` est fabriqué **par le propriétaire de la machine, à partir de sa propre image BIOS**
(`build_nav_bank.py`). Il contient des textes du fabricant du BIOS : il n'est pas redistribué.
Sans `NAV.BIN`, le lecteur utilise les menus que le firmware publie en direct (mode dégradé).

## Utilisation

Démarrer sur la clé. Touches :

| Touche | Action |
|---|---|
| Flèches haut / bas | élément précédent / suivant |
| Entrée | ouvrir le sous-menu |
| Échap ou Retour arrière | revenir au menu parent |
| Début / Fin | premier / dernier élément |
| Page haut / Page bas | section précédente / suivante |
| Gauche / Droite | écouter les options d'une liste de choix |
| H ou F1 | aide de l'élément |
| R | répéter |
| Espace | où suis-je (page, puis élément) |
| Échap deux fois au menu principal | quitter (le démarrage continue) |

**Lecture seule** : aucun réglage du BIOS n'est jamais écrit.

## Reproduire la release

### Binaire EFI

Chaîne d'outils figée : **LLVM 23.1.1** (clang + lld-link), identique sous Windows et Linux.

```
pwsh -File build_release.ps1 -OutDir out [-NavBin NAV.BIN]
```

Le script compile deux fois depuis zéro et exige deux binaires identiques octet pour octet,
puis vérifie le format exigé pour la signature UEFI : PE32+ x64, sous-système
EFI_APPLICATION, `NX_COMPAT`, `DYNAMIC_BASE`, sections alignées sur 4 Kio, aucune section
à la fois inscriptible et exécutable, relocations présentes et non retirées.
Il écrit `SHA256SUMS.TXT` et `BUILD-INFO.json` (commit source, versions, options).
La CI (`.github/workflows/uefi-nav-bin-ci.yml`) refait la même compilation sous Linux
avec LLVM 23.1.1 téléchargé depuis la release officielle (empreinte vérifiée).

### NAV.BIN

```
python build_nav_bank.py <image BIOS> NAV.BIN --st <chemin de st ou st.exe> --manifest NAV-MANIFEST.json
```

La voix est **celle d'omni-os** : la voix ST compacte (`voice-st`, licence 0BSD), déterministe,
donc un même BIOS donne toujours le même `NAV.BIN` à l'octet près. `NAV-MANIFEST.json` enregistre
l'empreinte de l'image BIOS et le moteur utilisé.

Audio : voix ST rendue à 48 kHz, ramenée à 16 kHz pour la banque, puis suréchantillonnée
16→48 kHz dans l'EFI par un filtre polyphasé (sinc fenêtré, 16 coefficients par phase,
images à −80 dB).

Option tierce, non utilisée par défaut : `--voice kokoro` (modèle Kokoro-82M, phonétisation
eSpeak NG sous GPL-3.0), avec son environnement Python passé par `--st-neural`.

## Sécurité

- `NAV.BIN` est traité comme une **entrée non fiable** : toutes les tailles sont vérifiées en
  64 bits (aucun débordement), chaque lien, cible, option et message système est validé avant
  usage ; au moindre défaut le fichier est rejeté et le lecteur passe en mode dégradé.
  Onze fichiers corrompus sont testés en CI (`test_nav_qemu.py --fuzz`).
- Aucune écriture de variable UEFI ni de réglage BIOS.
- Écritures matérielles limitées au contrôleur audio : registres HDA, commande PCI
  (mémoire + bus master) et, pour AMD/ATI, l'octet de configuration 0x42 (« snoop »,
  comme le pilote Linux) ; le cache est réécrit en mémoire (`wbinvd`) avant chaque DMA.
- Seul fichier écrit : `OMNI-SR-TRACE.TXT`, à la racine du volume de démarrage, taille fixe.

## Signature Microsoft (Secure Boot)

Pour démarrer avec Secure Boot activé, `SCREENREADER.EFI` doit être signé par l'autorité
UEFI tierce de Microsoft (« Microsoft UEFI CA »). Ce que ce dépôt fournit : binaire
reproductible, audit du format, tests, code source. Ce qui reste à faire **par le titulaire
du projet** (ce n'est pas automatisable) :

1. un compte entreprise sur le Microsoft Partner Center (programme matériel) ;
2. un certificat de signature de code **EV** pour signer la soumission ;
3. empaqueter `SCREENREADER.EFI` dans un fichier CAB signé EV et le soumettre comme
   « UEFI firmware submission » ;
4. répondre à la revue de sécurité de Microsoft (ce document, `BUILD-INFO.json` et le
   code source servent de dossier).

Seul le fichier `.EFI` est signé ; `NAV.BIN` est une donnée validée à l'exécution.
