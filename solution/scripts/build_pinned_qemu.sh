#!/usr/bin/env bash
set -euo pipefail

QEMU_VERSION="11.1.1"
QEMU_RELEASE_KEY_FPR="CEACC9E15534EBABB82D3FA03353C9CEF108B584"
QEMU_BASE_URL="https://download.qemu.org"
QEMU_TARBALL_SHA256="079ffbff8a7111bbc89022107cbabf3bbfd614d5fc9d7cc675991196aca12482"
QEMU_KEY_URL="https://keys.openpgp.org/vks/v1/by-fingerprint/${QEMU_RELEASE_KEY_FPR}"
QEMU_SLIRP_MODE="system"
QEMU_SLIRP_PACKAGE="libslirp-dev"

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 OUTPUT_QEMU_SYSTEM [OUTPUT_QEMU_IMG]" >&2
  exit 2
fi

OUTPUT="$(readlink -f "$1")"
IMG_OUTPUT=""
if [[ $# -eq 2 ]]; then
  IMG_OUTPUT="$(readlink -f "$2")"
fi

WORK="${RUNNER_TEMP:-/tmp}/omni-qemu-${QEMU_VERSION}"
rm -rf "$WORK"
mkdir -p "$WORK/gnupg"
chmod 700 "$WORK/gnupg"

TARBALL="qemu-${QEMU_VERSION}.tar.xz"
SIGNATURE="${TARBALL}.sig"
wget --https-only --quiet --output-document "$WORK/$TARBALL" "$QEMU_BASE_URL/$TARBALL"
wget --https-only --quiet --output-document "$WORK/$SIGNATURE" "$QEMU_BASE_URL/$SIGNATURE"
wget --https-only --quiet --output-document "$WORK/qemu-release-key.asc" "$QEMU_KEY_URL"

echo "$QEMU_TARBALL_SHA256  $WORK/$TARBALL" | sha256sum -c -

GNUPGHOME="$WORK/gnupg" gpg --batch --import "$WORK/qemu-release-key.asc"
ACTUAL_FPR="$(GNUPGHOME="$WORK/gnupg" gpg --batch --with-colons --fingerprint "$QEMU_RELEASE_KEY_FPR" | awk -F: '$1=="fpr" {print $10; exit}')"
if [[ "$ACTUAL_FPR" != "$QEMU_RELEASE_KEY_FPR" ]]; then
  echo "QEMU release-key fingerprint mismatch: expected $QEMU_RELEASE_KEY_FPR got $ACTUAL_FPR" >&2
  exit 1
fi
GNUPGHOME="$WORK/gnupg" gpg --batch --verify "$WORK/$SIGNATURE" "$WORK/$TARBALL"

tar -C "$WORK" -xJf "$WORK/$TARBALL"
SOURCE="$WORK/qemu-${QEMU_VERSION}"
test -d "$SOURCE"
test "$(tr -d '\r\n' < "$SOURCE/VERSION")" = "$QEMU_VERSION"

if ! pkg-config --exists slirp; then
  echo "missing required QEMU user-network dependency: $QEMU_SLIRP_PACKAGE" >&2
  exit 1
fi
SLIRP_VERSION="$(pkg-config --modversion slirp)"
test -n "$SLIRP_VERSION"

cd "$SOURCE"
# QEMU 11.1.x accepts --enable-slirp as a boolean feature switch. Because
# downloads are disabled above and libslirp is verified through pkg-config,
# this remains a fail-closed system-libslirp build without the obsolete
# --enable-slirp=system spelling.
./configure \
  --disable-download \
  --target-list=x86_64-softmmu \
  --disable-docs \
  --disable-gtk \
  --disable-sdl \
  --disable-opengl \
  --disable-curses \
  --disable-vnc \
  --enable-slirp

ninja -C build qemu-system-x86_64 qemu-img
test -x build/qemu-system-x86_64
test -x build/qemu-img
mkdir -p "$(dirname "$OUTPUT")"
cp build/qemu-system-x86_64 "$OUTPUT"

NETDEV_HELP="$("$OUTPUT" -netdev help 2>&1)"
if ! grep -Eq '(^|[[:space:]])user([[:space:]]|$)' <<<"$NETDEV_HELP"; then
  echo "QEMU user networking backend missing despite system libslirp configuration" >&2
  printf '%s\n' "$NETDEV_HELP" >&2
  exit 1
fi

DATA_DIR="${OUTPUT}.data"
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"
cp -a "$SOURCE/pc-bios/." "$DATA_DIR/"
for rom in kvmvapic.bin vgabios-stdvga.bin; do
  test -f "$DATA_DIR/$rom"
done

if [[ -n "$IMG_OUTPUT" ]]; then
  mkdir -p "$(dirname "$IMG_OUTPUT")"
  cp build/qemu-img "$IMG_OUTPUT"
fi

"$OUTPUT" --version
sha256sum "$OUTPUT"
sha256sum "$DATA_DIR/kvmvapic.bin" "$DATA_DIR/vgabios-stdvga.bin"
if [[ -n "$IMG_OUTPUT" ]]; then sha256sum "$IMG_OUTPUT"; fi
printf 'QEMU_SOURCE_VERSION=%s\nQEMU_RELEASE_KEY_FPR=%s\nQEMU_SLIRP_MODE=%s\nQEMU_SLIRP_VERSION=%s\nQEMU_DATA_DIR=%s\n' \
  "$QEMU_VERSION" "$QEMU_RELEASE_KEY_FPR" "$QEMU_SLIRP_MODE" "$SLIRP_VERSION" "$DATA_DIR"
