# Sécurité d'omni-os : confidentialité, intégrité, disponibilité

## Confidentialité

- Dépôt **privé**. On ne le rend public qu'après une relecture volontaire.
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
- **Protection des branches** : l'offre GitHub gratuite ne permet pas de règles de branche sur un
  dépôt privé. Compensation : `integrity daily` (tous les jours à 03:17 UTC) échoue si une
  branche `archive/*` est supprimée ou réécrite, et le bundle hors ligne permet de la restaurer.
  Avec GitHub Pro (ou si le dépôt devient public), activer : `main` sans force-push ni suppression,
  `archive/**` en lecture seule.
- `CODEOWNERS` : tout changement requiert le propriétaire.

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

Ouvrir une *issue* privée (Security advisory) sur le dépôt, ou contacter le propriétaire.
