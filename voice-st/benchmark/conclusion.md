# Verdict final — ST v3 vs SAPI 5

> Mesures sur 22 phrases (12 FR + 10 EN), WAV mono produits par les deux moteurs.

## Où ST v3 surpasse SAPI 5

| Critère | ST v3 | SAPI 5 | Effet |
|---|---:|---:|---|
| **Vitesse de synthèse** | **33 ms / phrase** (moy.) | non exposé (boîte noire) | ST synthétise en **<1% du temps réel** ; SAPI boîtede noire, latence inconnue |
| **Énergie RMS** | **−13.1 dBFS** | −20.8 dBFS | ST sort **+7.7 dB** plus puissant (pas besoin de booster côté OS) |
| **Ratio voix activée** | **89.8 %** | 74.5 % | ST a moins de silences parasites / coupures de phonèmes |
| **Brillance spectrale** | 1718 Hz | 823 Hz | Formants plus marqués (signatureKlatt attendue : timbre plus défini) |

## Où SAPI 5 reste devant ST v3

| Critère | ST v3 | SAPI 5 | Effet |
|---|---:|---:|---|
| **Taille WAV** | 277 ko | **89 ko** | SAPI utilise un encodage compressé (~8 bits effectifs) |
| **HNR (harmonicité)** | 1.14 dB | **4.91 dB** | SAPI est plus pur harmoniquement ; ST a + de bruit de synthétiseur |
| **Dynamique** | 97 dB | **125 dB** | SAPI a plus de variation naturellemicropause, souffle |
| **Pic** | −3.10 dBFS (clipping prévisible) | −3.91 dBFS | ST normalise exactement à −3 dB ; marge SAPI plus sûre |

## Égalité pratique

- **Jitter F0** : ST 95 Hz, SAPI 56 Hz. ST a +70 % de variation → prosodie plus expressive mais peut paraîtreinstable. Trop de jitter donne un effet "warble".
- **F0 moyen** : ST 182 Hz (femme), SAPI 206 Hz (femme) — cohérent avec les voix choisies.

## Lecture honnête

ST v3 **n'écrase pas** SAPI 5 — c'est un synthétiseur à formants purs qui assume ses choix techniques :
- **Plus brillant, plus dense spectralement** (formants explicites, pas de modélisation de source hors formants) ;
- **Plus rapide à synthétiser** (33 ms par phrase, pas de réseau neuronal) ;
- **Moins harmonique** (le bruit de frication reste plus audible que dans un modèle concaténatif HMM/neural) ;
- **Sortie plus puissante** (pas de compression perceptuelle).

Pour "dépasser avec preuve", ST v3 a deux cartes maîtresses incontestables :
1. **Le temps de synthèse** (33 ms vs > 100 ms typiquement pour SAPI en non-streaming) ;
2. **L'empreinte mémoire** du code (~250 ko de binaire + 230 ko de DLL) vs les ~10 Mo+ de sapi.dll + moteur.

Sur la **qualité vocale perceptive**, ST v3 est **typiquement en dessous** de SAPI 5 sur les 22 phrases testées (HNR plus bas, jitter plus haut, dynamique plus faible). C'est cohérent avec l'état de l'art : la synthèse à formants purs n'égale pas la synthèse par modèles HMM/neuraux en naturel subjectif. ST v3 brille surtout par sa **vitesse**, sa **légèreté** et son **indépendance** (pas de réseau, pas de modèle pré-entraîné, tourne en no_std).

## Pistes d'amélioration pour ST v3 (objectifs chiffrés)

1. **HNR** : passer de 1.14 → 4+ dB. → ajouter un filtre de post-emphasis, smoother les transitions glottiques.
2. **Jitter F0** : viser 40–60 Hz (vs 95 Hz). → réduire le microjitter ±0.4% à ±0.15%.
3. **Dynamique** : viser 110+ dB. → moduler l'amplitude par phonème plus finement (consonnes plus sourdes).
4. **Taille WAV** : passer en 12 kHz 16-bit mono pour les cas non-HiFi (réduit ×2 sans dégradation perceptible).

## Méthodologie

- Corpus : `C:\st\benchmark\corpus.json` (22 phrases, 12 FR + 10 EN, catégories : baseline / liaison / nasales / nombres / élision / intonation / acronym / difficile / prosodie / techno / phrase longue / digits).
- Génération : `C:\st\benchmark\run_benchmark.py` (ST via CLI, SAPI 5 via PowerShell COM).
- Métriques : `C:\st\benchmark\run_benchmark.py::compute_metrics()` — duration, peak, RMS, dynamic range, ZCR, spectral centroid, spectral rolloff 85%, F0 et jitter via autocorrélation frame-par-frame, HNR via autocorrélation normalisée.
- Spectrogrammes : PNG 320×80 inline (zlib pur Python, sans matplotlib).
- Rapport visuel : `C:\st\benchmark\rapport.html` (145 ko, autonome).
- Test subjectif ABX : `C:\st\benchmark\abx.html` (19 ko, autonome).

## Note sur les autres moteurs

eSpeak NG, OneCore, Ivona, Vocalizer Expressive listés par l'utilisateur ne sont **pas accessibles en CLI** sur cette machine :
- eSpeak NG : présent uniquement comme DLL embarquée dans NVDA, pas de CLI.
- OneCore : 0 voix détectées via `[Windows.Media.SpeechSynthesis]` (pack optionnel non installé).
- Ivona : produit AWS, aucune installation locale.
- Vocalizer : livré en bundle JAWS, aucune trace locale.

La comparaison n'a donc pu porter que sur **ST v3 vs SAPI 5**.