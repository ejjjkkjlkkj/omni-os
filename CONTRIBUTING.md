# Contribuer à omni-os

## Avant d'ouvrir une pull request

```bash
tools/boot/run-qemu.sh                                  # démarrage + preuves
cd os && cargo fmt --all --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace
cd voice-st && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
cd solution && PYTHONPATH=src python -m unittest discover -s tests
python tools/integrity/verify.py
```

La CI rejoue tout cela, plus un démarrage QEMU sur image disque GPT.

## Règles

- **Aucun avertissement** : rustfmt et clippy `-D warnings` sont bloquants.
- **Voix** : un changement du moteur ST qui modifie le son doit le dire. Sinon, prouver que
  les WAV générés sont identiques octet pour octet.
- **Démarrage** : un nouveau marqueur de preuve `AW_*` se déclare dans
  [`tools/boot/check-log.sh`](tools/boot/check-log.sh), jamais seulement dans un workflow.
- **Archives** : les branches `archive/**` sont en lecture seule ; on ne les modifie pas.
- **Secrets** : aucun jeton, clé ou mot de passe dans le dépôt (la CI et GitHub les refusent).
- Un commit de formatage pur va dans `.git-blame-ignore-revs`.

Les composants gardent aussi leurs propres guides : [`os/CONTRIBUTING.md`](os/CONTRIBUTING.md),
[`solution/CONTRIBUTING.md`](solution/CONTRIBUTING.md).
