# Security policy

## Scope

The project is for development and validation on systems we control. OMNI security and accessibility architecture covers cryptography, firmware, boot, networking, privacy, supply chain, identity, trusted accessibility paths, speech/braille privacy, recovery, and evidence handling.

## Reporting vulnerabilities

Do not publish exploitable vulnerability details, credentials, keys, tokens, recovery secrets, private evidence, or personal data in a public issue. Use GitHub Private Vulnerability Reporting when enabled. If unavailable, open only a minimal non-sensitive request to establish a private reporting channel.

## Secrets

Never commit API tokens, private keys, passwords or PINs, TPM-unsealed secrets, recovery keys, private certificates, or private user evidence.

## Security + accessibility

A security fix is incomplete if it makes the affected workflow inaccessible to keyboard-only, speech, or braille users.

An accessibility fix is incomplete if it exposes secrets, weakens authorization, bypasses integrity checks, or creates a spoofable security path.

## Firmware boundary

Production-host code must not expose arbitrary kernel/physical-memory read-write, token theft, unsigned-driver loading, Secure Boot bypass, OEM SMM injection, SPI-flash override, or PSP takeover primitives.

Deep firmware work belongs in our own OVMF/EDK II/QEMU lab. Physical OEM firmware is read/measure/validate by default.
