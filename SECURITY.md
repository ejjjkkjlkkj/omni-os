# Sécurité d'omni-os : confidentialité, intégrité, disponibilité

## Confidentialité

- Dépôt **public** depuis le 2026-09-28, après relecture : tout le contenu venait déjà de
  dépôts publics, sauf `st` (auteur en adresse noreply, aucune donnée personnelle).
- **Analyse des secrets GitHub** activée, avec **blocage au push** : un jeton poussé par erreur
  est refusé avant d'arriver dans l'historique.
- **Aucun secret dans le dépôt** : ni jeton, ni clé, ni mot de passe. Les identifiants
  des runners et des labs passent par les *secrets* GitHub Actions, jamais par des fichiers.
- Audit du 2026-09-28 sur tout l'historique (3 264 commits) : aucun secret, aucune adresse
  e-mail ni aucun numéro de série matériel. Les seules IP sont celles de QEMU (`10.0.2.x`)
  et des exemples de test.
- La CI vérifie l'absence de secret à chaque push (`tools/integrity/verify.py`).
- Les workflows s'exécutent avec `permissions: contents: read` et `persist-credentials: false`.

## Intégrité

- **Provenance verrouillée** : `tools/integrity/SOURCES.lock.json` enregistre, pour chaque
  composant, le dépôt source, le commit et l'empreinte d'arbre à l'import, ainsi que le SHA
  des 74 branches `archive/*`.
- **La CI échoue si** un commit source sort de l'historique, si une archive est supprimée
  ou déplacée, si un secret apparaît ou si un fichier dépasse 50 Mio.
- **Chaîne de build reproductible** : actions épinglées par SHA, `cargo --locked`, toolchain
  Rust épinglée (`os/rust-toolchain.toml`), lockfiles vérifiés.
- **Protection des branches** (règles GitHub actives, vérifiées par un push refusé) :
  - `main-integrity` : `main` ne peut être ni supprimée ni réécrite (pas de force-push) ;
  - `archive-read-only` : les branches `archive/**` ne peuvent être ni créées, ni modifiées,
    ni réécrites, ni supprimées.
- `integrity daily` (03:17 UTC) revérifie chaque jour les 74 archives et la provenance.
- `CODEOWNERS` : tout changement requiert le propriétaire.

- **Versions publiées** : construites seulement si toute la CI passe, démarrées dans QEMU
  avant publication, avec `SHA256SUMS` et attestation de provenance **SLSA Build L3** signée
  par le workflow isolé `build-attested.yml`
  (`gh attestation verify <fichier> -R ejjjkkjlkkj/omni-os --signer-workflow ejjjkkjlkkj/omni-os/.github/workflows/build-attested.yml`). Voir
  [docs/SECURITY-FRAMEWORK.md](docs/SECURITY-FRAMEWORK.md).

## Disponibilité

- **Trois copies** : GitHub (`ejjjkkjlkkj/omni-os`), un bundle git hors ligne vérifié
  avec son SHA-256 (`C:\OMNI-BACKUPS\omni-os\`, via `tools/backup.sh`) et les dépôts
  sources d'origine (non modifiés).
- **Restauration** (testée) : `sha256sum -c omni-os-<date>.bundle.sha256`, puis
  `git clone omni-os-<date>.bundle omni-os` ; les 75 branches sont sous `origin/*`, et
  `python tools/integrity/verify.py --archive-prefix refs/remotes/origin/` doit afficher PASS.
- Les binaires lourds (images disque, sorties `target/`) restent hors du dépôt : le clone
  reste léger (~30 Mio) et se régénère depuis les sources.
- Pas de dépendance réseau cachée au build : les crates sont figées par `Cargo.lock`.

## Signaler un problème

Utiliser le **signalement privé de vulnérabilités** (onglet *Security*, puis *Report a vulnerability*).
Ne pas ouvrir d'issue publique pour une faille.
