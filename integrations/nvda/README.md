# ST pour NVDA (pilote de synthèse)

`release\stSynth-<version>.nvda-addon` ajoute la synthèse « ST » à NVDA x64 (2025.1+,
testé contre l'API de NVDA 2026.3). Le son passe directement des blocs ST à `nvwave`
en mémoire, sans fichier WAV.

- **Voix compactes** (toujours disponibles, DLL incluse) : FR/EN femme/homme, premier
  son ~30 ms, timbre robotique.
- **Voix neuronales** (FR Siwis ; EN Heart, Michael, Emma) : disponibles quand le pilote
  trouve une release ST complète. Après installation, écrire le chemin de cette release
  (le dossier qui contient `st_synth.dll` et `neural\`) dans
  `%APPDATA%\nvda\addons\stSynth\synthDrivers\st_home.txt`, ou définir `ST_HOME`.
  Le modèle est chargé à la sélection de la voix (~3 s), pas à la première touche.
- Débit NVDA 0–100 → ST 50–200 % (neuronal) / 50–300 % (compact), sans rechargement.
- `cancel()` arrête ST (`st_engine_cancel_v1`) et le lecteur audio ; aucun index d'un
  énoncé annulé n'est émis après l'annulation.
- Si la voix neuronale ne peut pas démarrer, le pilote journalise l'erreur et parle
  avec la voix compacte de la même langue plutôt que de rester muet.

## Installation (à faire par l'utilisateur)

1. Ouvrir le fichier `.nvda-addon` : NVDA propose l'installation, puis redémarre.
2. NVDA → Préférences → Paramètres → Parole → Synthétiseur : choisir « ST ».
   Garder un synthétiseur de secours connu (eSpeak, OneCore) : NVDA y revient
   automatiquement si ST ne se charge pas.

## Tests

`python integrations\nvda\test_driver.py <dossier contenant st_synth.dll>` exerce le
pilote hors NVDA avec des stubs fidèles de l'API NVDA utilisée (nvwave, index, fin de
parole) et la vraie DLL : parole, index ordonnés, annulation, débit, repli compact.
`build.ps1` lance ce test sur les fichiers exacts du paquet avant de le produire.
Le test dans NVDA réel reste à faire : il demande d'installer l'add-on et de
redémarrer NVDA.
