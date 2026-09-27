# Security Policy

## Scope

This repository develops the security and accessibility architecture for OMNI.

Security reports may concern cryptography, firmware, boot, networking, privacy, supply-chain, identity, trusted accessibility paths, speech/braille privacy, recovery, or evidence handling.

## Reporting vulnerabilities

Do not publish exploitable vulnerability details, credentials, keys, tokens, recovery secrets, private evidence, or personal data in a public issue.

Use GitHub Private Vulnerability Reporting when enabled for this repository. If that feature is unavailable, open a public issue containing only a minimal non-sensitive request to establish a private reporting channel.

## Secrets

Never commit:

- API tokens;
- private keys;
- passwords or PINs;
- TPM-unsealed secrets;
- recovery keys;
- private certificates;
- private user evidence;
- unredacted accessibility logs containing sensitive text.

## Security + accessibility requirement

A security fix is incomplete if it makes the affected workflow inaccessible to keyboard-only, speech, or braille users.

An accessibility fix is incomplete if it exposes secrets, weakens authorization, bypasses integrity checks, or creates a spoofable security path.
