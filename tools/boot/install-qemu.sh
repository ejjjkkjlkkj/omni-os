#!/usr/bin/env bash
# Pre-installation environment proof, in QEMU (Linux and Windows):
#   0. forged   - the medium's kernel signature does not verify: refused before any write
#   1. install  - the machine boots an installation medium (\OMNI\INSTMED) with a blank 1 GiB
#                 NVMe disk inside; by keyboard the disk is chosen, named again with a warning,
#                 confirmed; GPT + FAT32 ESP written and read back, loader, kernel and boot
#                 state copied and verified by SHA-256, Boot#### "omni-os" first in BootOrder
#   2. installed - the medium is removed: the firmware boots omni-os from the NVMe disk through
#                 that boot option (BootCurrent), the loader finds the installer's boot state
#                 (known-good generation 1, no first-boot initialisation) and the kernel runs
#   tools/boot/install-qemu.sh [--no-build]        -> OMNI_OS_INSTALL=PASS or fails
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$ROOT"
PY="$(command -v python || command -v python3)"
B=build/install-proof; rm -rf "$B"; mkdir -p "$B"
fail() { echo "INSTALL PROOF FAILED: $*" >&2; exit 1; }
LOG=build/boot/boot.log
need() { grep -qF "$1" "$LOG" || fail "missing $1"; }
# Throwaway publisher key for this proof: the loader is built with its public half and the
# kernel is signed with omni-sign (the release key never leaves the publisher).
export OMNI_SIGNING_SEED="$("$PY" -c 'import secrets; print(secrets.token_hex(32))')"
(cd os && cargo build --release --quiet -p aw-sign --bin omni-sign)
SIGN="${CARGO_TARGET_DIR:-$ROOT/os/target}/release/omni-sign"
export OMNI_PUBLISHER_PUBKEY="$("$SIGN" public)"
WARMUP=(bash tools/boot/run-qemu.sh); [ "${1:-}" = "--no-build" ] && WARMUP+=(--no-build)
"${WARMUP[@]}" --esp "$B/reference.img" >/dev/null || fail "reference boot"
"$PY" tools/boot/fatimg.py read "$B/reference.img" EFI/BOOT/BOOTX64.EFI "$B/loader.efi" >/dev/null
"$PY" tools/boot/fatimg.py read "$B/reference.img" KERNEL.BIN "$B/kernel.bin" >/dev/null
printf 'omni-os installation medium\r\n' > "$B/instmed"
"$SIGN" kernel "$B/kernel.bin" "$B/kernel.sig"
"$SIGN" kernel "$B/loader.efi" "$B/wrong.sig"   # a valid signature, but of another file
"$PY" tools/boot/fatimg.py build "$B/medium.img" 64 "EFI/BOOT/BOOTX64.EFI=$B/loader.efi" \
  "KERNEL.BIN=$B/kernel.bin" "KERNEL.SIG=$B/kernel.sig" "OMNI/INSTMED=$B/instmed"
"$PY" tools/boot/fatimg.py build "$B/forged.img" 64 "EFI/BOOT/BOOTX64.EFI=$B/loader.efi" \
  "KERNEL.BIN=$B/kernel.bin" "KERNEL.SIG=$B/wrong.sig" "OMNI/INSTMED=$B/instmed"
export VARS_FD="$B/nvram.fd" NVME_IMG="$B/internal.img" NVME_SIZE=1G
A=AW_INSTALL_AWAITING_INPUT

# 0. A medium whose kernel signature does not verify: the installer refuses, nothing is written.
PORT=$(( 4600 + RANDOM % 300 ))
QEMU_EXTRA="-qmp tcp:127.0.0.1:$PORT,server=on,wait=off" bash tools/boot/run-qemu.sh --no-build \
  --esp "$B/forged.img" --timeout 900 >"$B/forged-run.txt" 2>&1 &
QEMU_RUN=$!
sleep 3
"$PY" tools/boot/qmp-keys.py "$PORT" "$LOG" 880 "$A|ret" "$A|ret" "AW_INSTALL_FAIL|" \
  || { kill "$QEMU_RUN" 2>/dev/null || true; fail "forged-signature keyboard session"; }
wait "$QEMU_RUN" || fail "the medium did not boot on after refusing (see $B/forged-run.txt)"
need "AW_PUBLISHER_KEY present=true"
need "AW_INSTALL_SIGNATURE verdict=invalid"
need "AW_INSTALL_FAIL reason=publisher_signature_invalid"
if grep -qF "AW_INSTALL_DISK_WRITTEN" "$LOG"; then fail "a forged image reached the disk"; fi
echo "forged signature: PASS (refused before any write)"

# 1. Install to the NVMe disk: Enter on the first disk, Enter again to confirm.
PORT=$(( 4600 + RANDOM % 300 ))
QEMU_EXTRA="-qmp tcp:127.0.0.1:$PORT,server=on,wait=off" bash tools/boot/run-qemu.sh --no-build \
  --esp "$B/medium.img" --timeout 900 >"$B/install-run.txt" 2>&1 &
QEMU_RUN=$!
sleep 3
"$PY" tools/boot/qmp-keys.py "$PORT" "$LOG" 880 "$A|ret" "$A|ret" "AW_INSTALL_DONE|" \
  || { kill "$QEMU_RUN" 2>/dev/null || true; fail "installer keyboard session"; }
wait "$QEMU_RUN" || fail "the medium did not boot on after installing (see $B/install-run.txt)"
for m in "AW_INSTALL_ENV disks=" "AW_INSTALL_CONFIRM_REQUIRED disk=" "NVMe" \
         "AW_INSTALL_DISK_WRITTEN esp_first=2048 esp_blocks=1048576" \
         "AW_INSTALL_FILE_VERIFIED path=EFI\\omni-os\\BOOTX64.EFI" \
         "AW_INSTALL_FILE_VERIFIED path=KERNEL.BIN" "AW_INSTALL_FILE_VERIFIED path=OMNI\\BOOTST.A" \
         "AW_INSTALL_SIGNATURE verdict=valid" "AW_INSTALL_FILE_VERIFIED path=KERNEL.SIG" \
         "AW_INSTALL_DONE" "boot_option=Boot" "AW_NATIVE_KERNEL_IDLE"; do
  need "$m"
done
cp "$LOG" "$B/install.log"
echo "install: PASS (disk chosen and confirmed by keyboard, GPT + FAT32 ESP, files verified, boot option registered)"

# 2. Medium removed: only the installed NVMe disk; same NVRAM (its boot options).
NO_BOOT_DISK=1 bash tools/boot/run-qemu.sh --no-build >"$B/installed-run.txt" 2>&1 \
  || fail "the installed system did not boot (see $B/installed-run.txt)"
need 'AW_UEFI_BOOT_CURRENT option=Boot'
need 'description="omni-os"'
need "AW_RECOVERY_BOOT generation=1 state=successful"
if grep -qF "AW_RECOVERY_STATE_INIT" "$LOG"; then fail "boot state not written by the installer"; fi
if grep -qF "AW_INSTALL_ENV" "$LOG"; then fail "the installed system offered to install again"; fi
cp "$LOG" "$B/installed.log"
echo "installed: PASS (firmware boot option omni-os, installer's boot state, kernel healthy)"
echo "OMNI_OS_INSTALL=PASS"
