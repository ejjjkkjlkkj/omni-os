# Voice + Navigation UEFI

Dépôt dédié aux deux sous-systèmes extraits de `ejjjkkjlkkj/accessible-windows` :

- **voice/** : synthèse vocale native VoiceCore, voix et tests associés.
- **navigation/** : navigation HII / lecteur d'écran UEFI temps réel et génération des unités vocales.

## Source figée

Extraction de la branche source `uefi-realtime-screenreader-20260919`, commit :

`4ae932d8a8dff533cfeb92156d49634fd060f84b`

## Périmètre exclu

Ce dépôt n'importe pas le reste du projet Accessible Windows : kernel, Secure Boot, TPM, stockage, USB/xHCI, image disque, boot manager générique, profils machine, runners physiques, pilotes et preuves non directement nécessaires à la voix ou à la navigation.

L'objectif est de faire évoluer la voix et la navigation indépendamment, puis de les réintégrer explicitement dans une plateforme UEFI lorsque leurs interfaces sont stables.
