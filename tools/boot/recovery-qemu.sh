#!/usr/bin/env bash
# Native Recovery Core proofs, on real FAT images booted in QEMU (Linux and Windows):
#   1. first boot   - no state: KERNEL.BIN becomes known-good generation 1, bound to its SHA-256
#   2. trial        - generation 2 on trial with 2 attempts is never promoted (no health proof),
#                     so it is tried twice, each attempt persisted before handoff, then the loader
#                     falls back to generation 1 by itself
#   3. tampered     - one byte of the kernel flipped: the loader refuses to run it, the Recovery
#                     Core announces the event, and a keyboard session starts the recovery
#                     medium found on a USB key (it returns to the menu), exports diagnostics
#                     and powers off only after explicit confirmation
#   tools/boot/recovery-qemu.sh [--no-build]        -> OMNI_OS_RECOVERY=PASS or fails
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$ROOT"
PY="$(command -v python || command -v python3)"
B=build/recovery-proof; rm -rf "$B"; mkdir -p "$B"
fail() { echo "RECOVERY PROOF FAILED: $*" >&2; exit 1; }
LOG=build/boot/boot.log
RUN=(bash tools/boot/run-qemu.sh --no-build)
# Reference boot: builds the loader and kernel once (unless --no-build) and yields the image
# the scenarios start from.
WARMUP=(bash tools/boot/run-qemu.sh); [ "${1:-}" = "--no-build" ] && WARMUP+=(--no-build)
"${WARMUP[@]}" --esp "$B/warmup.img" >/dev/null || fail "reference boot"
need() { grep -qF "$1" "$LOG" || fail "missing $1"; }
state() { "$PY" tools/boot/fatimg.py read "$1" "OMNI/BOOTST.$2" "$B/rec" >/dev/null 2>&1 && "$PY" tools/boot/bootstate.py decode "$B/rec"; }
K="$B/kernel.bin"; "$PY" tools/boot/fatimg.py read "$B/warmup.img" KERNEL.BIN "$K" >/dev/null

# 1. First boot.
"${RUN[@]}" --esp "$B/first.img"
need "AW_RECOVERY_STATE_INIT generation=1 persisted=true"
need "AW_RECOVERY_BOOT generation=1 state=successful"
state "$B/first.img" A | grep -q "sequence=1 selected=1 known_good=1 rollback_floor=1 state=successful" \
  || fail "first-boot record"
echo "first boot: PASS"

# 2. Trial generation, bounded attempts, automatic fallback.
"$PY" tools/boot/bootstate.py encode "$B/trial.bin" 5 2 "$K" 1 "$K" 1 trial 2
EFI_SRC="$B/loader.efi"; "$PY" tools/boot/fatimg.py read "$B/warmup.img" EFI/BOOT/BOOTX64.EFI "$EFI_SRC" >/dev/null
"$PY" tools/boot/fatimg.py build "$B/trial.img" 64 "EFI/BOOT/BOOTX64.EFI=$EFI_SRC" "KERNEL.BIN=$K" \
  "OMNI/GEN/2/KERNEL.BIN=$K" "OMNI/BOOTST.A=$B/trial.bin"
expect=("B:sequence=6 selected=2 known_good=1 rollback_floor=1 state=attempt tries=1"
        "B:sequence=8 selected=2 known_good=1 rollback_floor=1 state=attempt tries=0"
        "A:sequence=9 selected=1 known_good=1 rollback_floor=1 state=successful tries=0")
boots=("generation=2 state=trial_attempt" "generation=2 state=trial_attempt" "generation=1 state=successful")
for n in 0 1 2; do
  "${RUN[@]}" --esp "$B/trial.img"
  need "AW_RECOVERY_BOOT ${boots[$n]}"
  copy="${expect[$n]%%:*}"; want="${expect[$n]#*:}"
  state "$B/trial.img" "$copy" | grep -qF "$want" || fail "trial boot $((n+1)): copy $copy is not '$want'"
  echo "trial boot $((n+1)): PASS (${boots[$n]})"
done
need "AW_RECOVERY_TRIAL_INTERRUPTED generation=2"

# 3. Tampered kernel: refused, announced, driven by keyboard.
cp "$B/first.img" "$B/tampered.img"
"$PY" - "$K" "$B/tampered.bin" <<'EOF'
import sys
data = bytearray(open(sys.argv[1], "rb").read()); data[len(data) // 2] ^= 1
open(sys.argv[2], "wb").write(data)
EOF
"$PY" tools/boot/fatimg.py put "$B/tampered.img" KERNEL.BIN "$B/tampered.bin"
# External recovery medium: a USB key whose removable-media loader is tools/boot/external-recovery.c.
( cd "$B" && clang --target=x86_64-pc-windows-msvc -ffreestanding -nostdlib -fno-stack-protector \
    -mno-red-zone -O2 -Wall -Werror -c "$ROOT/tools/boot/external-recovery.c" -o ext.obj \
  && lld-link -subsystem:efi_application -entry:efi_main -nodefaultlib -machine:x64 -out:ext.efi ext.obj ) \
  >/dev/null || fail "external recovery medium build"
"$PY" tools/boot/fatimg.py build "$B/usbkey.img" 16 "EFI/BOOT/BOOTX64.EFI=$B/ext.efi"
USBKEY="-drive if=none,id=usbkey,format=raw,file=$ROOT/$B/usbkey.img -device usb-storage,drive=usbkey"
PORT=$(( 4600 + RANDOM % 300 ))
QEMU_EXTRA="-qmp tcp:127.0.0.1:$PORT,server=on,wait=off $USBKEY" "${RUN[@]}" --esp "$B/tampered.img" \
  --timeout 300 >"$B/tampered-run.txt" 2>&1 &
QEMU_RUN=$!
A=AW_RECOVERY_AWAITING_INPUT
sleep 3
# Menu: retry, previous, external recovery, diagnostics, reinstall, power-off. The focus stays on
# an action after it ran.
"$PY" tools/boot/qmp-keys.py "$PORT" "$LOG" 290 "$A|down" "$A|down" "$A|ret" \
  "AW_RECOVERY_EXTERNAL_RETURNED|" "$A|down" "$A|ret" \
  "AW_RECOVERY_DIAGNOSTICS_EXPORTED|" "$A|down" "$A|down" "$A|ret" "$A|ret" "AW_RECOVERY_POWER_OFF|" \
  || { kill "$QEMU_RUN" 2>/dev/null || true; fail "keyboard session"; }
wait "$QEMU_RUN" || true   # the run ends by power-off, before any kernel: that is the expected outcome
for m in "AW_RECOVERY_INTEGRITY_FAIL generation=1" "AW_RECOVERY_EVENT code=0x1102" \
         "AW_RECOVERY_EVENT_DELIVERED speech=true" "AW_RECOVERY_CORE_READY" \
         "AW_RECOVERY_FOCUS action=external_recovery" "AW_RECOVERY_EXTERNAL media=1" \
         "AW_RECOVERY_EXTERNAL_START" "AW_EXTERNAL_RECOVERY_RAN" "AW_RECOVERY_EXTERNAL_RETURNED status=None" \
         "AW_RECOVERY_FOCUS action=export_diagnostics" "AW_RECOVERY_DIAGNOSTICS_EXPORTED ok=true" \
         "AW_RECOVERY_CONFIRM_REQUIRED action=power_off" "AW_RECOVERY_POWER_OFF"; do
  need "$m"
done
if grep -qF AW_EXIT_BOOT_SERVICES_BEGIN "$LOG"; then fail "the tampered kernel was started"; fi
"$PY" tools/boot/fatimg.py read "$B/tampered.img" OMNI/DIAG.TXT "$B/diag.txt" >/dev/null || fail "no DIAG.TXT"
grep -q "name=object_verification_failed" "$B/diag.txt" || fail "DIAG.TXT content"
echo "tampered kernel: PASS (refused, announced, USB recovery started and returned, diagnostics exported, confirmed power-off)"
echo "OMNI_OS_RECOVERY=PASS"
