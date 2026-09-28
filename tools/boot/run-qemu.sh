#!/usr/bin/env bash
# Builds the native kernel and the UEFI loader, then boots them in QEMU (q35 + OVMF)
# and checks the proof markers. Works on Linux and on Windows (Git Bash + QEMU).
#   tools/boot/run-qemu.sh [--no-build] [--timeout 90]
# Output: build/boot/boot.log. Needs: rustup, qemu-system-x86_64, OVMF/edk2 firmware.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; OS="$ROOT/os"; OUT="$ROOT/build/boot"
BUILD=1; TIMEOUT=90
while [ $# -gt 0 ]; do case "$1" in
  --no-build) BUILD=0 ;; --timeout) TIMEOUT="$2"; shift ;;
  *) echo "unknown option $1" >&2; exit 2 ;; esac; shift; done

native() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
QEMU="$(command -v qemu-system-x86_64 || true)"
[ -n "$QEMU" ] || [ ! -x "/c/Program Files/qemu/qemu-system-x86_64.exe" ] || QEMU="/c/Program Files/qemu/qemu-system-x86_64.exe"
[ -n "$QEMU" ] || { echo "qemu-system-x86_64 not found" >&2; exit 1; }
CODE=""; VARS=""
for pair in /usr/share/OVMF/OVMF_CODE_4M.fd:/usr/share/OVMF/OVMF_VARS_4M.fd \
            /usr/share/OVMF/OVMF_CODE.fd:/usr/share/OVMF/OVMF_VARS.fd \
            "$(dirname "$QEMU")/share/edk2-x86_64-code.fd:$(dirname "$QEMU")/share/edk2-i386-vars.fd"; do
  c="${pair%%:*}"; v="${pair##*:}"; if [ -f "$c" ] && [ -f "$v" ]; then CODE="$c"; VARS="$v"; break; fi
done
[ -n "$CODE" ] || { echo "OVMF/edk2 firmware not found" >&2; exit 1; }

# Honour CARGO_TARGET_DIR (e.g. a short path on Windows, where MAX_PATH can bite).
KTARGET="${CARGO_TARGET_DIR:-$OS/kernel/x86_64/target}"; UTARGET="${CARGO_TARGET_DIR:-$OS/boot/uefi/target}"
KELF="$KTARGET/x86_64-unknown-none/release/aw-kernel-x86_64"
EFI="$UTARGET/x86_64-unknown-uefi/release/aw-uefi-boot.efi"
if [ "$BUILD" = 1 ]; then
  (cd "$OS" && rustup target add x86_64-unknown-uefi x86_64-unknown-none >/dev/null)
  (cd "$OS" && cargo build --locked --manifest-path kernel/x86_64/Cargo.toml --target x86_64-unknown-none --release)
  (cd "$OS" && cargo build --locked --manifest-path boot/uefi/Cargo.toml --target x86_64-unknown-uefi --release)
fi
OBJCOPY="$(command -v objcopy || true)"
if [ -z "$OBJCOPY" ]; then
  OBJCOPY="$(find "$(cd "$OS" && rustc --print sysroot)" -name 'llvm-objcopy*' -type f 2>/dev/null | head -1)"
fi
[ -n "$OBJCOPY" ] || { echo "objcopy not found (install binutils or rustup component llvm-tools)" >&2; exit 1; }

rm -rf "$OUT"; mkdir -p "$OUT/esp/EFI/BOOT"
cp "$EFI" "$OUT/esp/EFI/BOOT/BOOTX64.EFI"
"$OBJCOPY" -O binary "$KELF" "$OUT/esp/KERNEL.BIN"
cp "$VARS" "$OUT/vars.fd"; chmod u+w "$OUT/vars.fd"; truncate -s 32M "$OUT/nvme.img"

set +e
timeout "${TIMEOUT}s" "$QEMU" -machine q35 -cpu max -smp 2 -m 1024M \
  -display none -serial none -monitor none -no-reboot \
  -debugcon file:"$(native "$OUT/boot.log")" \
  -drive if=pflash,format=raw,readonly=on,file="$(native "$CODE")" \
  -drive if=pflash,format=raw,file="$(native "$OUT/vars.fd")" \
  -drive format=raw,file=fat:rw:"$(native "$OUT/esp")" \
  -drive if=none,format=raw,file="$(native "$OUT/nvme.img")",id=nvme0 -device nvme,drive=nvme0,serial=AWNVME \
  -device qemu-xhci -device ich9-intel-hda
STATUS=$?
set -e
if [ "$STATUS" -ne 0 ] && [ "$STATUS" -ne 124 ]; then echo "QEMU exited $STATUS" >&2; exit 1; fi
"$ROOT/tools/boot/check-log.sh" "$OUT/boot.log"
