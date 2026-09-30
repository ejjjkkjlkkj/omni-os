# omni-os face aux référentiels NIST et SLSA

Ce document situe omni-os par rapport aux publications du NIST qui encadrent le firmware
et le développement logiciel, et au cadre SLSA pour la provenance des binaires. Il dit ce
qui est couvert, ce qui ne l'est pas, et pourquoi.

**Périmètre.** omni-os s'exécute *sur* le firmware du constructeur : il ne peut pas protéger
la mémoire flash du BIOS ni son processus de mise à jour, qui relèvent du matériel et du
constructeur. Il agit sur ce qu'il contrôle : ce qu'il démarre, ce qu'il détecte et annonce,
comment il permet de récupérer, et la preuve de ce qu'il publie.

## Résilience du firmware : NIST SP 800-193

SP 800-193 organise la résilience en trois fonctions : **protéger**, **détecter**, **récupérer**.

| Fonction | Exigence (résumé) | omni-os | État |
|---|---|---|---|
| Protéger | le firmware et ses données critiques ne changent que par un mécanisme authentifié | aucune écriture du firmware ; les seules variables modifiées (`BootOrder`, `BootNext`, réglages) le sont sur action explicite de l'utilisateur, annoncée à voix haute ; `SecureBoot` n'est jamais modifié par le chargeur | fait, dans la limite du périmètre |
| Détecter | repérer une modification non autorisée avant exécution | **le noyau est vérifié par SHA-256 avant chaque exécution** : un seul octet modifié le fait refuser (`AW_RECOVERY_INTEGRITY_FAIL`) ; état Secure Boot, clés et TPM annoncés à chaque démarrage (`AW_UEFI_SECURITY`) ; **journal TCG rejoué et comparé aux PCR 0-7 du TPM, dérive depuis le démarrage précédent détectée et annoncée à voix haute** (`AW_UEFI_MEASURED`) ; noyau mesuré dans le PCR 9 | fait (attestation à distance à faire) |
| Récupérer | revenir à une version authentique | **Recovery Core natif** : génération à l'essai bornée avec retour automatique à la génération connue bonne, retour manuel, diagnostic, menu parlé au clavier ; image publiée avec empreinte et provenance | fait : promotion par le bilan de santé du noyau, réinstallation vérifiée par empreinte ; signature d'éditeur des images à faire |

## Protection et mesure du BIOS : NIST SP 800-147 et SP 800-155

| Référence | Sujet | omni-os |
|---|---|---|
| SP 800-147 | mises à jour du BIOS authentifiées et non contournables | hors périmètre : omni-os ne met pas à jour le firmware (capsules non implémentées) |
| SP 800-155 | mesurer l'intégrité du BIOS et en rendre compte | rendu compte : l'état de sécurité est annoncé à voix haute, ce qu'aucun firmware ne fait pour une personne aveugle ; la mesure est vérifiée : journal TCG rejoué contre le TPM, référence du démarrage précédent, écart annoncé |

## Développement sécurisé : NIST SP 800-218 (SSDF)

| Famille SSDF | Pratiques visées | omni-os |
|---|---|---|
| PO : préparer l'organisation | exigences de sécurité écrites, outillage | [SECURITY.md](../SECURITY.md), [CONTRIBUTING.md](../CONTRIBUTING.md), CI obligatoire |
| PS : protéger le logiciel | intégrité du code et des versions publiées | `main` protégée (ni force-push, ni suppression) ; 74 branches d'archive en lecture seule ; analyse des secrets avec blocage au push ; provenance verrouillée (`tools/integrity/SOURCES.lock.json`) ; empreintes SHA-256 et attestation signée de chaque version |
| PW : produire un logiciel sûr | revue, tests, analyse | rustfmt + clippy `-D warnings` bloquants ; tests unitaires ; C en gcc et clang `-Werror`, ASan/UBSan, analyseur statique ; démarrage réel dans QEMU ; navigation parlée avec audio vérifié au bit près ; toolchain et dépendances épinglées (`--locked`) |
| RV : répondre aux vulnérabilités | signalement, correction | signalement privé de vulnérabilités ; alertes et mises à jour Dependabot ; corrections tracées dans l'historique |

## Provenance des binaires : SLSA

Chaque version publiée par [`release.yml`](../.github/workflows/release.yml) :

- est construite sur les runners hébergés et éphémères de GitHub, à partir du commit tagué,
  **seulement si toute la CI passe** sur ce commit ;
- contient **tous les composants** du dépôt (chargeur, noyau, image disque, `NAVIGATION.EFI`,
  `SCREENREADER.EFI`, `OmniProbe.efi`, `OmniGuardianProbe.efi`, voix ST, wheel `solution`,
  archive source) ;
- a été **démarrée dans QEMU avant publication** : image GPT exacte, `NAVIGATION.EFI`,
  `SCREENREADER.EFI` (navigation, audio, 11 `NAV.BIN` corrompus rejetés), `OmniProbe.efi`
  (preuve relue sur le disque) et `OmniGuardianProbe.efi` (inventaire relu) ;
- est construite **et signée** par le workflow réutilisable
  [`build-attested.yml`](../.github/workflows/build-attested.yml), isolé du workflow qui le
  déclenche : l'attestation de provenance (SLSA / in-toto, via Sigstore) porte l'identité de
  ce workflow, que la vérification exige. C'est le niveau **SLSA v1.0 Build L3** tel que
  GitHub le définit (build hébergé, provenance signée non falsifiable par le code appelant),
  soit le niveau le plus élevé de la piste « build » de SLSA v1.0 ;
- **au-delà de SLSA : build reproductible.** Chaque binaire est reconstruit sur un second
  runner indépendant et doit être identique à l'octet près (fichier `REPRODUCIBLE`). N'importe
  qui peut donc reconstruire et comparer, sans faire confiance au service de build ;
- **aucun contenu propriétaire** : toutes les voix embarquées (chargeur, noyau, navigation,
  `NAV.BIN`) sont produites par les synthétiseurs d'omni-os ; les anciens clips issus de voix
  Windows ou d'un modèle tiers ont été remplacés, et leurs générateurs retirés ;
- **aucun outil propriétaire** dans la construction : rustc/LLVM, clang/lld, GCC mingw-w64,
  LLVM 23.1.1 officiel vérifié par empreinte, EDK II `edk2-stable202608` vérifié par commit,
  setuptools épinglé, sous Linux. La voix construite avec GCC et celle construite avec MSVC produisent les mêmes
  132 fichiers audio à l'octet près (`tools/voice/golden.sh`), vérifié en CI et à chaque version.

Vérifier une version téléchargée :

```bash
sha256sum -c SHA256SUMS
```

```bash
gh attestation verify BOOTX64.EFI -R ejjjkkjlkkj/omni-os --signer-workflow ejjjkkjlkkj/omni-os/.github/workflows/build-attested.yml
```

## Confidentialité, intégrité, disponibilité, prouvabilité

| Propriété | Mécanismes |
|---|---|
| Confidentialité | aucun secret dans le dépôt (vérifié en CI, blocage au push) ; réseau fermé par défaut dans le chargeur, aucun paquet émis (`AW_UEFI_NET … transmitted=0`) ; workflows en lecture seule |
| Intégrité | branches protégées, archives immuables, provenance verrouillée, builds `--locked`, attestations signées, état Secure Boot/TPM annoncé |
| Disponibilité | trois copies (GitHub, bundle hors ligne vérifié, dépôts d'origine) ; restauration testée ; réparation locale sans réseau (règle 9 de la récupération) ; audio sur tout matériel (HDA, AC'97, virtio, USB, haut-parleur) |
| Prouvabilité | builds reproductibles vérifiés à chaque version ; voix comparée à un corpus de référence de 132 phrases ; chaque affirmation technique est un marqueur `AW_*` exigé par la CI ; démarrage QEMU à chaque changement ; audio de navigation comparé au bit près ; expériences contrôlées documentées dans l'historique ; binaires publiés avec provenance vérifiable |

## Sources

- NIST SP 800-193, *Platform Firmware Resiliency Guidelines* (2018) :
  [publication](https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-193.pdf),
  [annonce](https://www.nist.gov/news-events/news/2018/05/nist-releases-special-publication-800-193-platform-firmware-resiliency)
- NIST SP 800-147, *BIOS Protection Guidelines* :
  [publication](https://nvlpubs.nist.gov/nistpubs/legacy/sp/nistspecialpublication800-147.pdf) ;
  SP 800-155, *BIOS Integrity Measurement Guidelines* (brouillon) :
  [publication](https://csrc.nist.gov/files/pubs/sp/800/155/ipd/docs/draft-sp800-155_dec2011.pdf)
- NIST SP 800-218, *Secure Software Development Framework* :
  [publication](https://nvlpubs.nist.gov/nistpubs/specialpublications/nist.sp.800-218.pdf) ;
  [mise en œuvre avec GitHub](https://wellarchitected.github.com/library/scenarios/nist-ssdf-implementation/)
- SLSA et GitHub : [attestations d'artefacts](https://docs.github.com/en/actions/concepts/security/artifact-attestations)
  (Build L2 par elles-mêmes, L3 avec un workflow réutilisable) ;
  [niveau 3 avec des workflows réutilisables](https://docs.github.com/actions/security-guides/using-artifact-attestations-and-reusable-workflows-to-achieve-slsa-v1-build-level-3)
