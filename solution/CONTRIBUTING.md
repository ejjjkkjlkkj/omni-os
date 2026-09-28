# Contributing

Contributions are welcome under the repository's 0BSD license.

## Development

Use Python 3.11 or newer. From the repository root:

```bash
export PYTHONPATH=src
python -m unittest discover -s tests -v
python -m omni.cli toolchain
```

On PowerShell:

```powershell
$env:PYTHONPATH = "src"
python -m unittest discover -s tests -v
python -m omni.cli toolchain
```

## Pull requests

Keep changes scoped and reproducible. Do not weaken fail-closed validation, exact-commit evidence binding, immutable action pinning, toolchain locks, formal checks, fuzzing, coverage requirements, or the separation between software proof and physical hardware evidence.

Changes to UEFI/SCT/QEMU/EDK II integration must preserve pinned upstream revisions and must include a regression test for compatibility shims or parser behavior.

Do not commit generated binaries, build trees, credentials, tokens, private keys, machine-specific secrets, or local evidence that cannot be reproduced.

## Security

Follow `SECURITY.md`. Do not submit code that adds arbitrary physical-memory write primitives, credential theft, Secure Boot bypass, unsigned-driver loading, OEM SMM injection, SPI-flash override, PSP takeover, or equivalent host-compromise mechanisms.
