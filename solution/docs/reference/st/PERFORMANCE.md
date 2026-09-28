# Performances mesurées — ST 0.6.0-rc.3

Machine : ASUS VivoBook M1603QA, AMD Ryzen 7 5800H (8C/16T), 32 Go, Windows 11,
CPU uniquement sauf mention. Toutes les valeurs sont des temps réels mesurés par
les outils du dépôt ; TTFA = temps entre l'appel et le premier bloc audio jouable.

## Navigation lecteur d'écran (`cargo run --release --example navigation`)

20 annonces au format « nom, rôle, état. », chacune interrompue par le focus suivant
dès son premier bloc audio.

| Configuration | P50 | P90 | P95 | P99 |
|---|---|---|---|---|
| Neural FR, cache vide (premier passage) | 362 ms | 507 ms | 530 ms | 553 ms |
| Neural FR, cache disque d'une session précédente | **6,3 ms** | 9,5 ms | 9,9 ms | 16,7 ms |
| Neural EN, cache vide | 360 ms | 422 ms | 428 ms | 480 ms |
| Neural EN, cache disque | **6,6 ms** | 12,2 ms | 20,3 ms | 24,1 ms |
| Neural FR après 45 s d'inactivité (préchauffage), cache vide | 337 ms | 514 ms | 516 ms | 568 ms |
| Compact FR | 88 ms | 119 ms | 130 ms | 142 ms |
| Compact EN | 64 ms | 116 ms | 118 ms | 123 ms |

Après préchauffage, les termes d'interface courants (« Fichier », « OK », « Annuler »,
« Paramètres », rôles et états) démarrent en **0,3–0,5 ms dès leur première occurrence** ;
les percentiles restent dominés par les noms propres à l'application, jamais vus.

## Interruption (`examples/latency.rs`, `tests/ffi.rs`, `tests/c/abi_test.c`)

| Mesure | Avant (rc.1) | rc.3 |
|---|---|---|
| Nouvel énoncé après interruption (neural, non caché) | 3 030 ms (rechargement du modèle) | ~400 ms |
| Nouvel énoncé après interruption (compact, binaire C) | — | 63 ms |
| Retour de `st_engine_cancel_v1` / du callback renvoyant 0 | — | immédiat (la purge a lieu au début de l'énoncé suivant) |

## Étapes neuronales (`scripts/neural_bench.py`, 58 énoncés, 2e passage)

| Étape | FP32 CPU (4 threads) |
|---|---|
| Import Python + ONNX Runtime | 0,85 s (une fois) |
| Création session ONNX | 1,16 s (une fois) |
| Chauffe (FR + EN) | 1,8 s (une fois) |
| Phonétisation eSpeak | 0,18 ms (P50) |
| Inférence | 393 ms (P50) |
| Rééchantillonnage 24→48 kHz | 1,6 ms (P50) |
| TTFA premier segment P50 / P90 / P95 / P99 | 395 / 674 / 1 233 / 1 565 ms |
| TTFA textes de 1–3 mots P50 / P99 | 388 / 533 ms |
| RTF P50 | 0,38 |

L'inférence représente > 99 % du TTFA d'un texte nouveau.

## Accélérations évaluées et rejetées

| Variante | Résultat | Décision |
|---|---|---|
| INT8 CPU (`kokoro-v1.0.int8.onnx`) | TTFA P50 5 685 ms, RTF 5,6 (14× plus lent) | rejetée |
| DirectML FP32 (Radeon intégrée) | échec natif (ConvTranspose groupé) | — |
| DirectML FP32, graphe réécrit | « Bouton » 541 ms, « Menu Fichier » 669 ms (CPU : ~350 ms) | rejetée |
| DirectML INT8 | plantage du fournisseur (0xC0000005) | rejetée |
| 8 threads au lieu de 4 | pas de gain (contention) | 4 threads |

## Robustesse (`examples/stress.rs`)

| Test | Résultat |
|---|---|
| Compact : 3 000 appels, 2/3 interrompus, 200 créations/destructions | 0 erreur, handles 70 → 70 |
| Neural : 600 appels, 2/3 interrompus, worker tué à mi-parcours | 1 appel en erreur puis reprise ; handles 79 → 80 ; aucun processus orphelin |
| Entrées invalides (vide, ponctuation seule, 70 Ko) | rejetées proprement |
| Modèle absent / corrompu / runtime absent | message explicite, code de sortie ≠ 0 |

## Conclusion pour un lecteur d'écran

- Moins de 100 ms en neuronal n'est atteint que depuis le cache. Sur ce CPU,
  Kokoro-82M ne produit pas un texte inédit en moins de ~300 ms.
- Le moteur compact tient la cible (< 150 ms au P99) pour un texte inédit.
- Recommandation d'intégration : un handle neural pour les annonces (cache disque et
  préchauffage actifs), un handle compact pour l'écho clavier caractère par caractère.
  ST ne change jamais de voix automatiquement.
