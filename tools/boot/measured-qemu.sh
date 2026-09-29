#!/usr/bin/env bash
# Boot integrity IDS proofs, in QEMU with an emulated TPM 2.0 (swtpm) and OVMF's measured boot:
#   1. first boot   - the TCG event log replays exactly to the TPM's PCR 0-7, the baseline
#                     \OMNI\PCR.REF is created, the kernel is measured into PCR 9, no alert
#   2. same machine - the PCRs equal the baseline: silence
#   3. drift        - a baseline whose PCR 4 (boot loader) differs: the change is detected,
#                     spoken, logged to \OMNI\IDS.LOG, and the baseline follows the new state
#   4. no TPM       - reported as such, the boot goes on
# Linux (swtpm).  tools/boot/measured-qemu.sh [--no-build]   -> OMNI_OS_MEASURED=PASS or fails
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$ROOT"
PY="$(command -v python || command -v python3)"
B=build/measured-proof; rm -rf "$B"; mkdir -p "$B/tpm"
LOG=build/boot/boot.log
fail() { echo "MEASURED BOOT PROOF FAILED: $*" >&2; exit 1; }
need() { grep -qF "$1" "$LOG" || { grep -F AW_UEFI_MEASURED "$LOG" >&2 || true; fail "missing $1"; }; }
never() { if grep -qF "$1" "$LOG"; then fail "unexpected $1"; fi; }
FIRST=(bash tools/boot/run-qemu.sh); [ "${1:-}" = "--no-build" ] && FIRST+=(--no-build)
SOCK="$ROOT/$B/swtpm.sock"
TPM_ARGS="-chardev socket,id=chrtpm,path=$SOCK -tpmdev emulator,id=tpm0,chardev=chrtpm -device tpm-tis,tpmdev=tpm0"

boot_with_tpm() {
  swtpm socket --tpm2 --tpmstate dir="$ROOT/$B/tpm" --ctrl type=unixio,path="$SOCK" \
    --flags startup-clear --daemon --pid file="$ROOT/$B/swtpm.pid"
  QEMU_EXTRA="$TPM_ARGS" "$@" --esp "$B/esp.img"
  kill "$(cat "$B/swtpm.pid")" 2>/dev/null || true
  rm -f "$SOCK"
}

# 1. First boot.
boot_with_tpm "${FIRST[@]}"
need "replay=match mismatched=none baseline=created changed=none"
need "complete=true"
need "AW_UEFI_MEASURED_KERNEL pcr=9"
need "ok=true"
never AW_UEFI_MEASURED_ALERT
"$PY" tools/boot/fatimg.py read "$B/esp.img" OMNI/PCR.REF > "$B/pcr.ref" || fail "no baseline written"
[ "$(wc -c < "$B/pcr.ref")" -eq 296 ] || fail "baseline has the wrong size"
echo "first boot: PASS (log replays to the TPM, baseline created, kernel measured)"

# 2. Same machine, same loader.
boot_with_tpm bash tools/boot/run-qemu.sh --no-build
need "replay=match mismatched=none baseline=same changed=none"
never AW_UEFI_MEASURED_ALERT
echo "second boot: PASS (unchanged, silent)"

# 3. Drift: a valid baseline in which PCR 4 (boot manager and loader code) differs.
"$PY" - "$B/pcr.ref" "$B/drift.ref" <<'EOF'
import hashlib, sys
data = bytearray(open(sys.argv[1], "rb").read())
data[8 + 32 * 4 : 8 + 32 * 5] = bytes(32)
data[-32:] = hashlib.sha256(bytes(data[:-32])).digest()
open(sys.argv[2], "wb").write(bytes(data))
EOF
"$PY" tools/boot/fatimg.py put "$B/esp.img" OMNI/PCR.REF "$B/drift.ref"
boot_with_tpm bash tools/boot/run-qemu.sh --no-build
need "replay=match mismatched=none baseline=changed changed=4"
need "AW_UEFI_MEASURED_ALERT mismatched=none changed=4"
need "registres 4. Le chargeur de d"
"$PY" tools/boot/fatimg.py read "$B/esp.img" OMNI/IDS.LOG | grep -qF "changed=4" || fail "alert not logged"
"$PY" tools/boot/fatimg.py read "$B/esp.img" OMNI/PCR.REF | cmp -s - "$B/pcr.ref" \
  || fail "the baseline did not follow the current state"
echo "drift: PASS (PCR 4 change detected, spoken, logged)"

# 4. No TPM.
bash tools/boot/run-qemu.sh --no-build --esp "$B/esp.img"
need "AW_UEFI_MEASURED tpm=absent"
echo "no TPM: PASS (reported, boot continues)"
echo "OMNI_OS_MEASURED=PASS"
