#!/usr/bin/env bash
# Boots the omni-os UEFI loader and native kernel in QEMU (q35 + OVMF, NVMe, xHCI and an
# HDA controller with a codec, so both the loader's and the kernel's speech really play),
# stops as soon as the kernel reports idle, then checks the proof markers.
# Works on Linux and on Windows (Git Bash + QEMU for Windows). Used by CI and developers.
#
#   tools/boot/run-qemu.sh [--no-build] [--disk IMAGE] [--timeout 420]
#     --no-build    reuse the last kernel/loader build
#     --disk IMAGE  boot this raw GPT image instead of an ESP folder built from the binaries
#     --timeout S   upper bound only; the run ends at AW_NATIVE_KERNEL_IDLE
#
# Output: build/boot/boot.log. Needs: rustup, qemu-system-x86_64, OVMF/edk2 firmware.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; OS="$ROOT/os"; OUT="$ROOT/build/boot"
BUILD=1; TIMEOUT=420; DISK=""
while [ $# -gt 0 ]; do case "$1" in
  --no-build) BUILD=0 ;;
  --timeout) TIMEOUT="$2"; shift ;;
  --disk) DISK="$2"; BUILD=0; shift ;;
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

rm -rf "$OUT"; mkdir -p "$OUT"
if [ -n "$DISK" ]; then
  [ -s "$DISK" ] || { echo "disk image not found: $DISK" >&2; exit 1; }
  BOOT_DRIVE="format=raw,file=$(native "$(cd "$(dirname "$DISK")" && pwd)/$(basename "$DISK")")"
else
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
  mkdir -p "$OUT/esp/EFI/BOOT"
  cp "$EFI" "$OUT/esp/EFI/BOOT/BOOTX64.EFI"
  "$OBJCOPY" -O binary "$KELF" "$OUT/esp/KERNEL.BIN"
  BOOT_DRIVE="format=raw,file=fat:rw:$(native "$OUT/esp")"
fi
cp "$VARS" "$OUT/vars.fd"; chmod u+w "$OUT/vars.fd"; truncate -s 32M "$OUT/nvme.img"
: > "$OUT/boot.log"

"$QEMU" -machine q35 -cpu max -smp 2 -m 1024M \
  -display none -serial none -monitor none -no-reboot \
  -debugcon file:"$(native "$OUT/boot.log")" \
  -drive if=pflash,format=raw,readonly=on,file="$(native "$CODE")" \
  -drive if=pflash,format=raw,file="$(native "$OUT/vars.fd")" \
  -drive "$BOOT_DRIVE" \
  -drive if=none,format=raw,file="$(native "$OUT/nvme.img")",id=nvme0 -device nvme,drive=nvme0,serial=AWNVME \
  -device qemu-xhci -device ich9-intel-hda -audiodev none,id=snd0 -device hda-output,audiodev=snd0 &
QPID=$!
trap 'kill "$QPID" 2>/dev/null || true' EXIT

# End the run at kernel idle (or at the upper bound); a QEMU that exits by itself is an error.
end=$(( $(date +%s) + TIMEOUT )); state=timeout
while [ "$(date +%s)" -lt "$end" ]; do
  if grep -qF AW_NATIVE_KERNEL_IDLE "$OUT/boot.log" 2>/dev/null; then state=idle; break; fi
  if ! kill -0 "$QPID" 2>/dev/null; then state=exited; break; fi
  sleep 1
done
sleep 1  # let the last lines reach the debug console
kill "$QPID" 2>/dev/null || true; wait "$QPID" 2>/dev/null || true; trap - EXIT
echo "OMNI_OS_BOOT_RUN=$state seconds=$(( TIMEOUT - (end - $(date +%s)) ))"
[ "$state" != exited ] || { echo "QEMU exited before the kernel reached idle" >&2; exit 1; }
"$ROOT/tools/boot/check-log.sh" "$OUT/boot.log"
