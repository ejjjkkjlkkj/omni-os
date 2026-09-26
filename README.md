# ST 0.5.0 — synthèse vocale FR/EN

Moteur de synthèse par formants en Rust, autonome, sans téléchargement de voix.
Sortie WAV PCM **32 000 Hz, 16 bits, mono**. Trois voix (male, female, child),
quatre qualités (modal, breathy, pressed, creaky), français et anglais.

## Utilisation Windows

```powershell
.\st.exe --text "Bonjour, le système est prêt." --out bonjour.wav --play
.\st.exe --lang en --voice female --text "Hello world." --out hello.wav
.\st.exe --rate 150 --pitch 130 --text "Les amis arrivent."
.\st.exe --demo
.\st.exe --version
.\st.exe --help
```

Sans --out, le fichier est speech_output.wav dans le dossier courant.
--demo produit demo_fr.wav et demo_en.wav; ajouter --play pour les écouter.
Les fichiers existants de même nom sont remplacés. --stdin lit du texte UTF-8.
Les erreurs de paramètres, d'écriture et de lecture audio renvoient un code non nul.
La lecture audio intégrée utilise winmm sur Windows.

## SSML limité

Le moteur accepte speak, break, emphasis, prosody et voice, avec attributs entre
guillemets doubles. Ce parseur ne constitue pas une implémentation SSML complète.

```xml
<speak>Bonjour <break time="250ms"/><voice name="female">les amis</voice>.</speak>
<prosody rate="slow" pitch="high">Attention.</prosody>
```

Les pauses sont plafonnées à 60 secondes chacune. Les préréglages de voix règlent
également la hauteur; une prosody imbriquée peut ensuite la modifier.

## Compilation et tests

```powershell
cargo build --release --locked
cargo test --release --locked -- --test-threads=1
```

Les réglages du moteur sont globaux. Sérialiser les appels de synthèse et les
changements de réglages entre threads, y compris via la DLL. Les tests unitaires
historiques modifient cet état et sont donc exécutés sur un seul thread.
La compatibilité UEFI/no_std du paquet complet n'est pas validée par cette release.

## Interface C

Voir include/st_synth.h. st_synthesize_wav reçoit du texte UTF-8 et renvoie un
buffer WAV; st_free_wav doit le libérer exactement une fois avec sa longueur.
Les pointeurs doivent rester valides pendant l'appel. Une erreur d'entrée renvoie
NULL et une longueur nulle. rate/pitch à zéro conservent les réglages courants.
Les plages sont 50–300 % et 50–350 Hz (valeurs positives bornées par le moteur).

## Limites et qualité

Le son reste celui d'un synthétiseur à formants. Cette release corrige la fidélité
de lecture des démos et des comportements de configuration; elle ne démontre
pas une supériorité perceptive sur SAPI ou une voix neuronale. Les anciens rapports
benchmark sont historiques et ne valident pas cette version. Une comparaison
subjective à volume égal reste nécessaire pour juger le naturel et l'intelligibilité.


## Changements 0.5.0 et comparaison

Correction des consonnes initiales anglaises (bought, thought, would, could…),
lexique courant enrichi, grands entiers jusqu'à 12 chiffres, maintien des zéros
initiaux par épellation, ponctuation et mots composés mieux séparés. Liaisons
françaises corrigées pour « on », les frontières de ponctuation et h muet/aspiré.
Les silences inter-mots systématiques sont supprimés. L'accent français se place
sur la dernière syllabe du mot (approximation, pas une analyse des groupes prosodiques).
Chaque phrase dispose de son propre contour terminal au lieu d'un contour global.

Ouvrir ECOUTER.html : 22 phrases, quatre versions/moteurs en ordre masqué,
volume rapproché, révélation des identités et export des préférences. Les sons
sont intégrés à la page. Les mesures détaillées figurent dans COMPARISON.json.
L'eSpeak local indique 1.52.0 ; il n'a pas été recompilé depuis l'archive fournie.
Microsoft désigne ici les voix Desktop Hortense et Zira, pas toutes ses technologies.

Résultats : 29 tests réussis ; 88 WAV de comparaison valides. HNR moyen :
ST 0.5.0 15,23 dB, ST 0.4.1 15,25 dB, eSpeak 11,33 dB, Microsoft 15,46 dB.
Le HNR mesure l'harmonicité estimée, pas la qualité perçue ni l'intelligibilité.
Aucune préférence d'écoute n'a encore été recueillie. Le corpus historique ne
contient pas tous les accents français ; ces résultats restent limités à ce corpus.
Les nombres décimaux, dates et abréviations ne disposent pas encore d'une analyse
contextuelle complète. Les homographes anglais restent ambigus.
