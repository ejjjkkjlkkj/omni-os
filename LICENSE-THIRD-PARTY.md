# Composants tiers

ST se compose de deux parties distinctes, avec des obligations de licence différentes.

## 1. Moteur compact (`st.exe`, `st_synth.dll`)

Code Rust de ST. Dépendances compilées : `libm` (MIT/Apache-2.0), `serde_json`,
`serde`, `serde_core`, `itoa`, `zmij`, `memchr` (MIT/Apache-2.0 ; `serde_derive`,
`syn`, `quote`, `proc-macro2`, `unicode-ident` servent seulement à la compilation). Aucun composant GPL.
Le moteur compact fonctionne seul, sans le dossier `neural\`.

## 2. Backend neuronal optionnel (`neural\`)

Exécuté dans un **processus séparé** (`neural\python\python.exe neural\worker.py`),
qui communique avec ST uniquement par un protocole binaire sur stdin/stdout (STN1).
Supprimer le dossier `neural\` retire tous les composants ci-dessous ; ST reste
alors pleinement fonctionnel avec le moteur compact.

| Composant | Licence | Rôle |
|---|---|---|
| Kokoro-82M v1.0 (poids ONNX `kokoro-v1.0.onnx`, `voices-v1.0.bin`) | Apache-2.0 | modèle acoustique + vocodeur |
| kokoro-onnx 0.6.1 | MIT | chargement du modèle |
| ONNX Runtime 1.30 | MIT | inférence CPU |
| NumPy 2.5 | BSD-3-Clause (+ 0BSD, MIT, Zlib, CC0) | calcul |
| **phonemizer / phonemizer-fork** | **GPL-3.0** | phonétisation |
| **eSpeak NG** (via `espeakng-loader`) | **GPL-3.0** | phonétisation (lexique, règles) |
| CPython 3.13 | PSF-2.0 | interpréteur |
| attrs, colorlog, dlinfo, jsonschema, language-tags, pyparsing, rpds-py, six, termcolor, referencing | MIT | dépendances |
| babel, joblib, rdflib, protobuf, python-dateutil | BSD | dépendances |
| csvw, flatbuffers, segments, rfc3986, regex, packaging, uritemplate | Apache-2.0 / BSD | dépendances |

Sources des poids : https://github.com/thewh1teagle/kokoro-onnx/releases/tag/model-files-v1.1
(sommes SHA-256 dans `neural\models.json`, vérifiées au démarrage).

### Obligation GPL-3.0

La redistribution du dossier `neural\` inclut phonemizer et eSpeak NG sous GPL-3.0.
Toute distribution doit fournir le texte de la GPL-3.0 et une offre de code source
correspondant pour ces composants. L'isolation en processus séparé est conçue
pour que le code de ST et du lecteur d'écran ne soit pas lié à du code GPL, mais
**la conformité doit être validée juridiquement avant toute diffusion commerciale**.
Une alternative sans GPL exige de remplacer la phonétisation (lexique + règles
maison produisant l'alphabet de Kokoro) ; ce travail n'est pas fait.

## Références qualité

Les enregistrements JAWS / Vocalizer présents sous `work\` servent uniquement de
référence de mesure locale. Aucun composant, voix, donnée ou enregistrement
Vocalizer/JAWS n'est inclus dans ST ni dans ses paquets.
