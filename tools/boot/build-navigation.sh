#!/usr/bin/env bash
# Builds NAVIGATION.EFI (spoken BIOS navigation) with open tools only: clang and lld-link
# (LLVM). The --target triple only selects the PE/COFF object format UEFI requires.
# Deterministic: /timestamp:0, no build paths embedded. Used by CI and by releases.
#   tools/boot/build-navigation.sh OUTDIR     -> OUTDIR/NAVIGATION.EFI
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UEFI="$ROOT/navigation/navigation/uefi"; B="${1:?usage: build-navigation.sh OUTDIR}"
mkdir -p "$B"; B="$(cd "$B" && pwd)"
cd "$ROOT/navigation"  # the generators resolve their data relative to the component root
python3 "$UEFI/generate_units.py" "$B/navigation_units.c" "$B/navigation_units.txt"
flags='--target=x86_64-pc-windows-msvc -DQEV_INTERACTIVE_NAV=1 -ffreestanding -fshort-wchar -fno-stack-protector -fno-builtin -mno-red-zone -nostdlib -O2 -Wall -Wextra -Werror'
# shellcheck disable=SC2086
clang $flags -c "$UEFI/semantic_core.c" -o "$B/semantic_core.obj"
# shellcheck disable=SC2086
clang $flags -I "$UEFI" -c "$UEFI/hii_graph_prompt_speech_uefi.c" -o "$B/navigation.obj"
# shellcheck disable=SC2086
clang $flags -I "$UEFI" -c "$B/navigation_units.c" -o "$B/navigation_units.obj"
lld-link /subsystem:efi_application /entry:efi_main /nodefaultlib /machine:x64 /timestamp:0 \
  /out:"$B/NAVIGATION.EFI" "$B/navigation.obj" "$B/navigation_units.obj" "$B/semantic_core.obj"
test -s "$B/NAVIGATION.EFI"
