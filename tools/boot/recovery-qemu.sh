#!/usr/bin/env bash
# Native Recovery Core proofs, on real FAT images booted in QEMU (Linux and Windows):
#   1. first boot   - no state: KERNEL.BIN becomes known-good generation 1, bound to its SHA-256
#   2. trial        - generation 2 on trial with 2 attempts, on a machine whose NVRAM is reset
#                     at every boot: the kernel's health record never survives, so the attempt is
#                     never promoted; it is tried twice, each attempt persisted before handoff,
#                     then the loader falls back to generation 1 by itself
#   2b. promotion   - the same trial with a persistent NVRAM: the kernel records its runtime
#                     health for the exact attempt (UEFI variable, runtime services), the next boot
#                     verifies it and promotes generation 2 to known-good; a forged record for
#                     another attempt is refused
#   4. reinstall    - generation 1 tampered and no other generation: reinstall from a USB key,
#                     verified against the recorded digest, announced target, confirmation, read
#                     back, then the restored system boots
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
# Throwaway publisher key for this proof: the loader is built with its public half and the
# kernel is signed with omni-sign (the release key never leaves the publisher).
export OMNI_SIGNING_SEED="$("$PY" -c 'import secrets; print(secrets.token_hex(32))')"
(cd os && cargo build --release --quiet -p aw-sign --bin omni-sign)
SIGN="${CARGO_TARGET_DIR:-$ROOT/os/target}/release/omni-sign"
export OMNI_PUBLISHER_PUBKEY="$("$SIGN" public)"
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

# 2b. Promotion by the kernel's runtime-health record (persistent NVRAM, as on real hardware).
"$PY" tools/boot/fatimg.py build "$B/promote.img" 64 "EFI/BOOT/BOOTX64.EFI=$EFI_SRC" "KERNEL.BIN=$K" \
  "OMNI/GEN/2/KERNEL.BIN=$K" "OMNI/BOOTST.A=$B/trial.bin"
export VARS_FD="$B/nvram.fd"; rm -f "$VARS_FD"
"${RUN[@]}" --esp "$B/promote.img"
need "AW_RECOVERY_BOOT generation=2 state=trial_attempt"
need "AW_UEFI_RUNTIME_READY mode="
need "AW_HEALTH_RECORDED generation=2 sequence=6"
echo "promotion boot 1: PASS (trial attempt, health recorded by the kernel through runtime services)"
"${RUN[@]}" --esp "$B/promote.img"
need "AW_RECOVERY_HEALTH_RECORD bytes=32 deleted=true"
need "AW_RECOVERY_PROMOTED generation=2 sequence=6"
need "AW_RECOVERY_BOOT generation=2 state=successful"
state "$B/promote.img" A | grep -qF "sequence=7 selected=2 known_good=2 rollback_floor=1 state=successful" \
  || fail "promoted record"
if grep -qF "AW_HEALTH_RECORDED" "$LOG"; then fail "health recorded outside a trial attempt"; fi
echo "promotion boot 2: PASS (health verified, generation 2 promoted to known-good)"

# 2c. One-shot update request (rules 3 and 4): generation 2, signed by the publisher, is dropped
#     next to a settled generation 1 with \OMNI\UPDATE.REQ; the loader consumes the request,
#     verifies the signature, stages a bounded trial, and health promotes it. A request whose
#     signature is forged is refused and generation 1 keeps booting.
"$SIGN" kernel "$K" "$B/update.sig"
printf 'generation=2\r\n' > "$B/update.req"
cp "$B/first.img" "$B/update.img"
for f in "OMNI/GEN/2/KERNEL.BIN=$K" "OMNI/GEN/2/KERNEL.SIG=$B/update.sig" "OMNI/UPDATE.REQ=$B/update.req"; do
  "$PY" tools/boot/fatimg.py put "$B/update.img" "${f%%=*}" "${f#*=}"
done
export VARS_FD="$B/nvram-update.fd"; rm -f "$VARS_FD"
"${RUN[@]}" --esp "$B/update.img"
need "AW_UPDATE_REQUEST consumed=true"
need "AW_UPDATE_STAGED generation=2 tries=2 signature=valid"
need "AW_RECOVERY_BOOT generation=2 state=trial_attempt"
need "AW_HEALTH_RECORDED generation=2"
if "$PY" tools/boot/fatimg.py read "$B/update.img" OMNI/UPDATE.REQ >/dev/null 2>&1; then fail "update request not consumed"; fi
"${RUN[@]}" --esp "$B/update.img"
need "AW_RECOVERY_PROMOTED generation=2"
need "AW_RECOVERY_BOOT generation=2 state=successful"
echo "update: PASS (request consumed, signature verified, trial, health, promoted)"
"$SIGN" kernel "$EFI_SRC" "$B/forged.sig"   # valid signature of another file
cp "$B/first.img" "$B/forged-update.img"
for f in "OMNI/GEN/2/KERNEL.BIN=$K" "OMNI/GEN/2/KERNEL.SIG=$B/forged.sig" "OMNI/UPDATE.REQ=$B/update.req"; do
  "$PY" tools/boot/fatimg.py put "$B/forged-update.img" "${f%%=*}" "${f#*=}"
done
"${RUN[@]}" --esp "$B/forged-update.img"
need "AW_UPDATE_REFUSED reason=publisher_signature_invalid generation=2"
need "AW_RECOVERY_BOOT generation=1 state=successful"
echo "forged update: PASS (refused, generation 1 kept)"
unset VARS_FD

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
USBKEY="-drive if=none,id=usbkey,format=raw,file=$ROOT/$B/usbkey.img -device usb-storage,drive=usbkey,bootindex=1"
PORT=$(( 4600 + RANDOM % 300 ))
ESP_BOOTINDEX=0 QEMU_EXTRA="-qmp tcp:127.0.0.1:$PORT,server=on,wait=off $USBKEY" "${RUN[@]}" --esp "$B/tampered.img" \
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

# 4. Verified reinstall from a USB key: the only generation is tampered; the key carries the good
#    image, which must match the recorded digest. Menu: retry, previous, external, diagnostics,
#    reinstall (4 x down), Enter, confirm with Enter.
cp "$B/first.img" "$B/reinstall.img"
"$PY" tools/boot/fatimg.py put "$B/reinstall.img" KERNEL.BIN "$B/tampered.bin"
"$SIGN" kernel "$K" "$B/kernel.sig"
"$PY" tools/boot/fatimg.py build "$B/reinstallkey.img" 16 "OMNI/REINST/KERNEL.BIN=$K" \
  "OMNI/REINST/KERNEL.SIG=$B/kernel.sig"
KEY="-drive if=none,id=rkey,format=raw,file=$ROOT/$B/reinstallkey.img -device usb-storage,drive=rkey"
PORT=$(( 4600 + RANDOM % 300 ))
ESP_BOOTINDEX=0 QEMU_EXTRA="-qmp tcp:127.0.0.1:$PORT,server=on,wait=off $KEY" "${RUN[@]}" --esp "$B/reinstall.img" \
  --timeout 400 >"$B/reinstall-run.txt" 2>&1 &
QEMU_RUN=$!
sleep 3
"$PY" tools/boot/qmp-keys.py "$PORT" "$LOG" 390 "$A|down" "$A|down" "$A|down" "$A|down" "$A|ret" "$A|ret" \
  "AW_RECOVERY_REINSTALLED|" || { kill "$QEMU_RUN" 2>/dev/null || true; fail "reinstall keyboard session"; }
wait "$QEMU_RUN" || fail "the reinstalled system did not boot (see $B/reinstall-run.txt)"
for m in "AW_RECOVERY_INTEGRITY_FAIL generation=1" "AW_RECOVERY_READINESS ready=true" \
         "AW_RECOVERY_FOCUS action=signed_reinstall" "AW_RECOVERY_CONFIRM_REQUIRED action=signed_reinstall" \
         "AW_RECOVERY_REINSTALL_TARGET disk=" "AW_RECOVERY_INTEGRITY_OK generation=1" \
         "AW_RECOVERY_REINSTALL_CANDIDATE signature=valid matches_known_good=true" \
         "AW_RECOVERY_REINSTALLED generation=1" "signature=valid" \
         "AW_RECOVERY_BOOT generation=1 state=recovery action=signed_reinstall" \
         "AW_NATIVE_KERNEL_IDLE"; do
  need "$m"
done
"$PY" tools/boot/fatimg.py read "$B/reinstall.img" KERNEL.BIN "$B/reinstalled.bin" >/dev/null
cmp -s "$B/reinstalled.bin" "$K" || fail "reinstalled KERNEL.BIN differs from the known-good image"
echo "reinstall: PASS (verified image from USB, target announced, confirmed, read back, booted)"
echo "OMNI_OS_RECOVERY=PASS"
