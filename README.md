# ST 0.6.0-rc.1 — synthèse vocale FR/EN pour lecteur d'écran

Deux moteurs derrière une seule interface (CLI, API Rust, ABI C) :

| Backend | Voix | Qualité | Dépendances | Latence (Ryzen 7 5800H, CPU) |
|---|---|---|---|---|
| **neural** | `ff_siwis` (FR) ; `af_heart`, `af_bella`, `am_michael`, `bf_emma`, `bm_george` (EN) | voix neuronale Kokoro-82M | dossier `neural\` (~500 Mo, Python privé) | chargement 2,6–9 s une fois ; premier son 0,35–1,8 s (médiane 0,84 s, corpus v2) ; RTF ~0,33 |
| **compact** | `male`, `female`, `child` + qualités `modal/breathy/pressed/creaky` | synthèse par formants (robotique) | aucune | premier son ~0,1–0,25 s, sans chargement |

Sortie : **PCM 48 kHz, 24 bits, mono** (WAV) ou flux `float` 48 kHz par blocs.

## Ligne de commande

```powershell
.\st.exe --lang fr --voice ff_siwis --text "Bonjour, bienvenue dans ST." --out test.wav
.\st.exe --lang en --voice af_heart --text "Hello, welcome to ST." --out hello.wav
.\st.exe --lang fr --voice female --text "Menu Fichier." --out compact.wav --play
.\st.exe --help
```

Le backend est déduit de la voix (`xx_nom` → neural) ou forcé par `--backend compact|neural`.
`--rate 50..200` (neural) ou `50..300` (compact). `--pitch` et `--quality` : compact uniquement.
Le temps de chargement, de premier son et de synthèse est écrit sur stderr (`ST_TIMING`).

Le runtime neuronal est cherché dans `<dossier de st.exe>\neural\` (`python\python.exe`,
`worker.py`, `models\`). Variables de développement : `ST_NEURAL_HOME`, `ST_PYTHON`.

## Intégration dans un lecteur d'écran (ABI C v1)

Voir `include/st_synth.h` et `examples/c/st_example.c`.

```c
StEngine *e;
const char *cfg = "{\"backend\":\"neural\",\"lang\":\"fr\",\"voice\":\"ff_siwis\"}";
st_engine_create_v1((const uint8_t*)cfg, strlen(cfg), &e);   /* une fois, au démarrage */
st_engine_stream_v1(e, (const uint8_t*)txt, strlen(txt), on_audio, ctx); /* par énoncé */
st_engine_destroy_v1(e);
```

- `on_audio(const float *pcm, size_t n, uint32_t rate, void *ctx)` reçoit des blocs 48 kHz
  dès que la première phrase est prête ; **retourner 0 interrompt la parole** (code 4).
- Codes : 0 OK, 1 entrée invalide, 2 erreur moteur, 3 occupé/panique, 4 annulé.
  `st_last_error_v1(buf, cap)` donne le message (par thread).
- Un handle = une voix ; appels sérialisés par handle ; plusieurs handles peuvent
  fonctionner en parallèle. Créer le handle neuronal au démarrage du lecteur d'écran
  pour ne payer le chargement qu'une fois ; garder un handle compact en secours.
- `st_engine_wav_v1` renvoie un WAV complet, libéré par `st_free_wav(ptr, len)`.
- L'ancienne API `st_synthesize_wav` (PCM16/32 kHz, réglages globaux) reste disponible.

API Rust : `st_synth::engine::{Engine, Options, Backend}` (`stream`, `synthesize`, `wav`).

## Normalisation du texte

Dates (`26/09/2026`, `2024-02-29`), heures (`14h05`, `14:05`), décimaux (`12,5` / `3.14`),
titres (`M.`, `Mme`, `Dr.`, `Mr.`) sont développés avant synthèse. Les formats ambigus
ou invalides restent littéraux. SSML limité (`speak`, `break`, `emphasis`, `prosody`,
`voice`) : moteur compact uniquement.

## Compilation, tests, paquet

```powershell
cargo build --release
cargo test --release                  # 36 tests ; le test neuronal s'exécute si
                                      # ST_PYTHON et ST_NEURAL_HOME sont définis
powershell -File scripts\package.ps1  # release\st-<version>-windows-x64\ + auto-test
```

Benchmark : `benchmark\run_benchmark.py --tag <nom> --engines st st-neural sapi5`, puis
`benchmark\compare_references.py --run <nom>` (mesures comparées aux captures JAWS/Vocalizer
locales, qui ne sont jamais copiées dans le produit).

## Limites connues

- Une seule voix neuronale française (`ff_siwis`, féminine).
- Phonétisation neuronale via eSpeak NG/phonemizer (**GPL-3.0**, processus séparé) :
  voir `LICENSE-THIRD-PARTY.md` avant toute diffusion.
- Cache audio par segment (64 Mo, ST_NEURAL_CACHE_MB) : un énoncé ou segment déjà prononcé démarre en ~0,1 ms. Mesure : cargo run --release --example latency.
- Premier son neuronal d'un énoncé nouveau 0,3–0,85 s (médiane 0,45 s, énoncés d'interface) : plus lent qu'un moteur embarqué type Vocalizer (< 0,1 s).
  Pour l'écho clavier, utiliser le handle compact.
- SSML non pris en charge par le backend neuronal ; pas de réglage de hauteur neuronal.
- Windows x64 uniquement pour le backend neuronal ; CPU seulement.
