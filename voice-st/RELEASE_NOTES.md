# ST 0.6.0-rc.3 — latence réelle, robustesse, frontend

- Cache neuronal à deux niveaux (mémoire + disque persistant) ; annonces depuis le cache :
  TTFA P50 6,3 ms / P99 16,7 ms (contre 362 / 553 ms à froid).
- Découpage des annonces courtes à chaque virgule ; préchauffage du vocabulaire d'interface
  pendant l'inactivité ; statistiques de cache (`Engine::neural_stats`).
- INT8 et DirectML mesurés et rejetés (plus lents sur cette machine) : `docs/PERFORMANCE.md`.
- Frontend : chemins Windows, URLs, e-mails, IPv4, pourcentages, montants, ordinaux FR, extensions.
- Vrai test de l'ABI C compilé avec gcc (`tests/c/abi_test.c`), exécuté par le script de paquet.
- Stress et injection de fautes (`examples/stress.rs`) ; benchmarks `navigation`, `latency`,
  `scripts/neural_bench.py` (P50–P99 par étape).
- Paquet : `MANIFEST.json`, `SBOM.cdx.json` (CycloneDX), `ABI_TEST.txt`, matrice de licences.
- Test d'écoute à l'aveugle : `scripts/perceptual_pack.py`.
- `st_engine_set_rate_v1` : débit modifiable sans recharger le modèle.
- Add-on NVDA `stSynth` (`integrations/nvda`) : parole en mémoire via nvwave, index, annulation
  sans index périmé, préchargement de la voix, repli compact.
# ST 0.6.0-rc.2 — comportement lecteur d'écran

- Interruption sans rechargement : l'énoncé suivant démarre en ~0,4 s au lieu de ~3 s.
- `st_engine_cancel_v1` : arrêt thread-safe depuis le thread clavier.
- Cache audio par segment (64 Mo) : énoncés répétés en ~0,1 ms.
- `examples/latency.rs` : mesure du TTFA en moteur persistant, avec interruption.
- 37 tests.

# ST 0.6.0-rc.1 — NextGen

- Backend neuronal optionnel (Kokoro-82M, Apache-2.0) exécuté dans un processus isolé
  avec runtime Python privé ; vérification SHA-256 des poids ; aucun téléchargement.
- Streaming par phrase : premier son neuronal ~0,4–0,55 s au lieu de 4,4 s ; RTF ~0,3 CPU.
- Master 48 kHz / 24 bits avec dither TPDF pour tous les backends (au lieu de 32 kHz / 16 bits).
- API Rust par instance (`engine::Engine`) et ABI C v1 par handle : création, streaming
  avec annulation, WAV, destruction, message d'erreur par thread.
- Synthèse compacte réentrante : `synth::Config` explicite, plus aucune mutation des
  réglages globaux pendant le SSML (corrige des courses entre threads).
- Normalisation FR/EN : dates, heures, décimaux, titres.
- CLI : `--voice` accepte les voix neuronales, `--backend`, mesures `ST_TIMING`.
- Tests : 36 (unitaires, ABI C, streaming = batch, annulation, neuronal conditionnel).
- Paquet autonome : `scripts\package.ps1`.

Limites et licences : voir README.md et LICENSE-THIRD-PARTY.md.
