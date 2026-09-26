# Composants tiers et licences

ST se compose de deux parties aux obligations différentes. Le **moteur compact**
(`st.exe`, `st_synth.dll`) ne contient aucun composant GPL. Le **backend neuronal**
(`neural\`) est optionnel, s'exécute dans un processus séparé et communique avec ST
uniquement par le protocole binaire STN1 sur stdin/stdout ; supprimer `neural\`
retire tous les composants de la section 2, et ST reste fonctionnel en compact.

État audité le 2026-09-27 à partir des métadonnées des paquets installés.
Ceci est un inventaire technique, pas un avis juridique.

## 1. Moteur compact (lié statiquement dans st.exe / st_synth.dll)

| Composant | Version | Fonction | Licence | Source | Redistribution | Alternative / remplacement |
|---|---|---|---|---|---|---|
| ST (synthèse formants, frontend, ABI) | 0.6.0 | moteur, normalisation, API | propriétaire du projet | ce dépôt | oui | — |
| libm | 0.2 | fonctions mathématiques | MIT / Apache-2.0 | crates.io | oui, avis de licence | std |
| serde, serde_core, serde_json | 1.x | configuration JSON de l'ABI | MIT / Apache-2.0 | crates.io | oui, avis | parseur maison |
| itoa, zmij, memchr | — | dépendances de serde_json | MIT / Apache-2.0 (memchr : Unlicense/MIT) | crates.io | oui, avis | — |
| serde_derive, syn, quote, proc-macro2, unicode-ident | — | compilation uniquement | MIT / Apache-2.0 | crates.io | non distribués | — |

## 2. Backend neuronal optionnel (dossier `neural\`)

| Composant | Version | Fonction | Licence | Source | Redistribution | Alternative | Remplacement possible |
|---|---|---|---|---|---|---|---|
| Kokoro-82M v1.0 (`kokoro-v1.0.onnx`, FP32) | v1.0 | modèle acoustique + vocodeur | Apache-2.0 | github.com/thewh1teagle/kokoro-onnx, release model-files-v1.1 (SHA-256 dans `models.json`) | oui, avec avis | — | autre modèle ; demande réentraînement |
| Voix Kokoro (`voices-v1.0.bin`) | v1.0 | styles de voix (ff_siwis, af_heart…) | Apache-2.0 | idem | oui, avec avis | — | — |
| kokoro-onnx | 0.6.1 | chargement du modèle, tokenisation | MIT | PyPI | oui | code maison (~300 lignes utiles) | oui, facile |
| ONNX Runtime (CPU) | 1.30.0 | inférence | MIT | PyPI / Microsoft | oui | — | ONNX Runtime C API sans Python |
| NumPy | 2.5.3 | calcul, rééchantillonnage | BSD-3-Clause (+ 0BSD, MIT, Zlib, CC0) | PyPI | oui | — | Rust |
| **phonemizer / phonemizer-fork** | 3.4.0 / 3.3.2 | appel d'eSpeak NG | **GPL-3.0** | PyPI | oui **sous GPL-3.0** (texte fourni, offre de source) | appel direct d'eSpeak | oui : couche fine |
| **eSpeak NG** (via espeakng-loader) | 1.52 (DLL + données) | phonétisation texte → IPA | **GPL-3.0** | github.com/espeak-ng/espeak-ng | oui **sous GPL-3.0** | G2P maison FR/EN | oui à terme : lexique + règles produisant l'IPA Kokoro ; **pas encore fait** |
| espeakng-loader | 0.2.4 | localisation de la DLL eSpeak | MIT | PyPI | oui | — | trivial |
| CPython | 3.13 | interpréteur privé | PSF-2.0 | python.org | oui, avis PSF | — | runtime Rust + ONNX Runtime C |
| attrs, colorlog, dlinfo, jsonschema, language-tags, pyparsing, rpds-py, six, termcolor, referencing | — | dépendances | MIT | PyPI | oui, avis | — | disparaissent avec phonemizer |
| babel, joblib, rdflib, protobuf, python-dateutil | — | dépendances | BSD | PyPI | oui, avis | — | idem |
| csvw, flatbuffers, segments, rfc3986, regex, packaging, uritemplate | — | dépendances | Apache-2.0 / BSD / PSF | PyPI | oui, avis | — | idem |

## 3. Évalués, non livrés

| Composant | Résultat mesuré (Ryzen 7 5800H) | Décision |
|---|---|---|
| `kokoro-v1.0.int8.onnx` (Apache-2.0, SHA-256 ae315a79…ee9c vérifié) | inférence CPU **14× plus lente** que FP32 | non livré |
| onnxruntime-directml 1.24.4 (MIT ; DirectML fait partie de Windows) | échec natif sur ConvTranspose groupé ; après réécriture du graphe (`scripts/make_dml_model.py`) fonctionne mais **plus lent** que CPU FP32 pour les textes courts | non livré |

## Obligation GPL-3.0

La redistribution du dossier `neural\` inclut phonemizer et eSpeak NG sous GPL-3.0 :
le paquet fournit `licenses\GPL-3.0.txt` ; une offre de code source correspondant
doit accompagner toute diffusion. L'isolation en processus séparé est conçue pour
que ST et le lecteur d'écran ne soient pas liés à du code GPL, mais **la conformité
doit être validée juridiquement avant toute diffusion commerciale**.

## Références qualité

Les enregistrements JAWS / Vocalizer présents sous `work\` servent uniquement de
référence de mesure locale. Aucun composant, voix, donnée ou enregistrement
Vocalizer/JAWS n'est inclus dans ST, dans ses paquets ni dans le test d'écoute.
