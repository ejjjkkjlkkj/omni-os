# Signature d'éditeur des images omni-os

Les images entrent dans un système omni-os par trois portes, et chacune vérifie la signature
Ed25519 de l'éditeur quand le chargeur porte une clé :

| Porte | Fichier signé | Refus |
|---|---|---|
| Pré-environnement d'installation ([`install.rs`](../os/boot/uefi/src/install.rs)) | `KERNEL.BIN` + `KERNEL.SIG` sur le support | signature invalide ou absente : rien n'est écrit |
| Réinstallation du Recovery Core ([`recovery.rs`](../os/boot/uefi/src/recovery.rs)) | `\OMNI\REINST\KERNEL.BIN` + `KERNEL.SIG` | signature invalide ; sans signature, seule l'image identique à la génération connue bonne est acceptée |
| Récupération par le réseau ([`net.rs`](../os/boot/uefi/src/net.rs)) | `<url>` + `<url>.sig` (`recover <url>`) | signature invalide ou absente ; `recover <url> sha256=…` garde l'empreinte fixée par le propriétaire |

- **Algorithme** : Ed25519 (RFC 8032), implémenté sans code tiers dans
  [`aw-sign`](../os/crates/aw-sign/) avec SHA-512 (FIPS 180-4). Les tests reproduisent octet pour
  octet les vecteurs officiels de la RFC 8032 et refusent toute signature altérée.
- **Séparation des domaines** : le message signé est `omni-os-kernel-v1` ou
  `omni-os-recovery-v1` suivi du SHA-256 de l'image ; une signature de noyau ne vaut jamais pour
  une image de récupération, ni l'inverse.
- **Confiance** : la clé publique est incluse dans le chargeur à la compilation
  (`OMNI_PUBLISHER_PUBKEY`) ; le chargeur lui-même est couvert par Secure Boot.
- **Preuves** : `install`, `recovery` et `network` génèrent une clé jetable à chaque exécution,
  signent avec [`omni-sign`](../os/crates/aw-sign/src/bin/omni-sign.rs) et prouvent les cas
  valides et falsifiés.

## Mettre en place la clé de publication

La clé privée ne quitte jamais l'éditeur. Une seule fois :

```bash
python -c "import secrets; print(secrets.token_hex(32))"
```

Garder cette valeur secrète, puis :

1. l'enregistrer comme secret GitHub `OMNI_SIGNING_SEED` du dépôt ;
2. écrire la clé publique dans le dépôt :
   `OMNI_SIGNING_SEED=<valeur> cargo run -q --release -p aw-sign --bin omni-sign public > os/keys/publisher.pub`
   (depuis `os/`), puis la commiter.

Dès que `os/keys/publisher.pub` existe, [`build-assets.sh`](../tools/release/build-assets.sh)
compile le chargeur avec cette clé, signe `KERNEL.BIN` (`KERNEL.SIG` publié à côté) et vérifie la
signature ; une publication sans le secret échoue. Ed25519 étant déterministe, la reconstruction
indépendante produit le même `KERNEL.SIG`.
