#!/usr/bin/env bash
# Builds every release asset with open-source toolchains only, on Linux:
#   rustc/LLVM (loader, kernel, voice), clang + lld-link (NAVIGATION.EFI),
#   mingw-w64 GCC (voice for Windows / WinPE / WinRE: x86_64-pc-windows-gnu),
#   pinned LLVM 23.1.1 (SCREENREADER.EFI), pinned EDK II + GCC (OmniProbe), setuptools (wheel).
# Deterministic, so an independent rebuild must produce byte-identical files.
#   tools/release/build-assets.sh OUTDIR
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:?usage: build-assets.sh OUTDIR}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
WIN=x86_64-pc-windows-gnu

# UEFI loader and kernel. When the repository carries the publisher's public key
# (os/keys/publisher.pub, docs/PUBLISHER-SIGNING.md), the loader embeds it and the kernel must be
# signed with the matching seed (OMNI_SIGNING_SEED, a release secret); Ed25519 is deterministic,
# so the signed release stays byte-identical across independent builds.
cd "$ROOT/os"
if [ -f keys/publisher.pub ]; then
  OMNI_PUBLISHER_PUBKEY="$(tr -d ' \r\n' < keys/publisher.pub)"; export OMNI_PUBLISHER_PUBKEY
  [ -n "${OMNI_SIGNING_SEED:-}" ] || { echo "os/keys/publisher.pub is set: OMNI_SIGNING_SEED is required" >&2; exit 1; }
fi
cargo build --locked --manifest-path kernel/x86_64/Cargo.toml --target x86_64-unknown-none --release
cargo build --locked --manifest-path boot/uefi/Cargo.toml --target x86_64-unknown-uefi --release
objcopy -O binary kernel/x86_64/target/x86_64-unknown-none/release/aw-kernel-x86_64 "$OUT/KERNEL.BIN"
cp boot/uefi/target/x86_64-unknown-uefi/release/aw-uefi-boot.efi "$OUT/BOOTX64.EFI"
if [ -n "${OMNI_PUBLISHER_PUBKEY:-}" ]; then
  cargo build --locked --release --quiet -p aw-sign --bin omni-sign
  [ "$(target/release/omni-sign public)" = "$OMNI_PUBLISHER_PUBKEY" ] \
    || { echo "OMNI_SIGNING_SEED does not match os/keys/publisher.pub" >&2; exit 1; }
  target/release/omni-sign kernel "$OUT/KERNEL.BIN" "$OUT/KERNEL.SIG"
  target/release/omni-sign verify "$OMNI_PUBLISHER_PUBKEY" "$OUT/KERNEL.BIN" "$OUT/KERNEL.SIG"
fi

# Spoken BIOS navigation.
"$ROOT/tools/boot/build-navigation.sh" "$ROOT/build/navigation-release"
cp "$ROOT/build/navigation-release/NAVIGATION.EFI" "$OUT/NAVIGATION.EFI"

# Voice ST: Linux, then Windows through mingw-w64.
cd "$ROOT/voice-st"
cargo build --locked --release
cp target/release/st "$OUT/st-linux-x86_64"
cp target/release/libst_synth.so "$OUT/libst_synth.so"
cargo build --locked --release --target "$WIN"
cp "target/$WIN/release/st.exe" "target/$WIN/release/st_synth.dll" "$OUT/"
cp "target/$WIN/release/libst_synth.dll.a" "$OUT/"
cp include/st_synth.h "$OUT/"
cargo build --locked --release --target "$WIN" --manifest-path integrations/sapi5/Cargo.toml
cp "integrations/sapi5/target/$WIN/release/st_sapi.dll" "$OUT/"

# The Windows binaries must only import DLLs that ship with Windows and WinPE/WinRE.
allowed='^(kernel32|ntdll|msvcrt|advapi32|ole32|oleaut32|user32|winmm|ws2_32|bcrypt|bcryptprimitives|userenv|shell32|api-ms-win-[a-z0-9-]+)\.dll$'
for f in st.exe st_synth.dll st_sapi.dll; do
  imports="$(x86_64-w64-mingw32-objdump -p "$OUT/$f" | awk '/DLL Name:/ {print tolower($3)}' | sort -u)"
  echo "$f imports: $(echo $imports)"
  bad="$(printf '%s\n' "$imports" | grep -Ev "$allowed" || true)"
  [ -z "$bad" ] || { echo "$f imports a non-system DLL: $bad" >&2; exit 1; }
done

# UEFI screen reader (pinned LLVM), OmniProbe (pinned EDK II), solution toolkit, source archive.
"$ROOT/tools/release/build-screenreader.sh" "$OUT"
"$ROOT/tools/release/build-probe.sh" "$OUT"
"$ROOT/tools/release/build-python.sh" "$OUT"
echo "OMNI_OS_ASSETS=BUILT files=$(ls "$OUT" | wc -l)"
