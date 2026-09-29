#!/usr/bin/env bash
# SCREENREADER.EFI with the pinned, checksum-verified LLVM 23.1.1 (open source, the compiler the
# component's reproducible release was defined with), through its own build_release.ps1: two
# clean builds must match and the PE image is audited (x64 EFI app, NX, W^X, relocations).
# Also a demonstration NAV.BIN built from the synthetic BIOS tree (real ones are made by the
# machine's owner from their own firmware and are never redistributed).
#   tools/release/build-screenreader.sh OUTDIR
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PY="$(command -v python || command -v python3)"
OUT="${1:?usage: build-screenreader.sh OUTDIR}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
SR="$ROOT/uefi-screenreader/boot/uefi-hii-graph-prompt-speech-v1"
LLVM="$ROOT/build/llvm-23.1.1"
ARCHIVE=LLVM-23.1.1-Linux-X64.tar.xz
SHA=832aeb58d105de1cabc7b982dd2c65de0610f7377df48ae8fc2dd8e97420a15c
if [ ! -x "$LLVM/bin/clang" ]; then
  mkdir -p "$LLVM"
  curl -fsSL -o "$ROOT/build/$ARCHIVE" "https://github.com/llvm/llvm-project/releases/download/llvmorg-23.1.1/$ARCHIVE"
  echo "$SHA  $ROOT/build/$ARCHIVE" | sha256sum -c -
  tar -xJf "$ROOT/build/$ARCHIVE" -C "$LLVM" --strip-components=1 \
    --wildcards '*/bin/clang*' '*/bin/lld*' '*/lib/clang/*'
  rm "$ROOT/build/$ARCHIVE"
fi
pwsh -NoProfile -File "$SR/build_release.ps1" -OutDir "$ROOT/build/screenreader-release" -Llvm "$LLVM/bin"
cp "$ROOT/build/screenreader-release/SCREENREADER.EFI" "$OUT/"
cp "$ROOT/build/screenreader-release/BUILD-INFO.json" "$OUT/SCREENREADER-BUILD-INFO.json"
"$PY" "$SR/build_nav_bank.py" synthetic "$OUT/NAV-DEMO.BIN" >/dev/null
