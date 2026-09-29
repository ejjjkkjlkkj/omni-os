#!/usr/bin/env bash
# Pre-boot network proofs (deny by default), in QEMU with its user-mode network:
#   1. no request  - interfaces are discovered read-only, no DHCP, nothing transmitted
#   2. request     - a one-shot \OMNI\NET.REQ left on the ESP is consumed before acting, then
#                    DHCP through the firmware stack yields QEMU's lease (10.0.2.15/24,
#                    gateway 10.0.2.2, DNS 10.0.2.3); the request is gone afterwards
#   3. recovery    - a request that pins a SHA-256 downloads a recovery image over HTTP,
#                    verifies it, starts it through LoadImage; it returns to the loader
#   4. tampered    - the same download with a different pinned digest is refused
#   tools/boot/network-qemu.sh [--no-build]         -> OMNI_OS_NETWORK=PASS or fails
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$ROOT"
PY="$(command -v python || command -v python3)"
B=build/network-proof; rm -rf "$B"; mkdir -p "$B"
LOG=build/boot/boot.log
fail() { echo "NETWORK PROOF FAILED: $*" >&2; exit 1; }
need() { grep -qF "$1" "$LOG" || fail "missing $1"; }
FIRST=(bash tools/boot/run-qemu.sh); [ "${1:-}" = "--no-build" ] && FIRST+=(--no-build)

# 1. No request: the network stays closed.
"${FIRST[@]}" --esp "$B/esp.img"
need "AW_UEFI_NET nics=1 policy=deny-by-default transmitted=0"
if grep -qF AW_UEFI_NET_DHCP_BEGIN "$LOG"; then fail "DHCP ran without a request"; fi
echo "no request: PASS (closed)"

# 2. One-shot request.
printf 'dhcp\n' > "$B/req"
"$PY" tools/boot/fatimg.py put "$B/esp.img" OMNI/NET.REQ "$B/req"
bash tools/boot/run-qemu.sh --no-build --esp "$B/esp.img"
need "AW_UEFI_NET_REQUEST consumed=true"
need "AW_UEFI_NET_DHCP_BEGIN reason=request_file"
need "AW_UEFI_NET_DHCP_OK address=10.0.2.15 mask=255.255.255.0 gateway=10.0.2.2 dns=10.0.2.3"
if "$PY" tools/boot/fatimg.py read "$B/esp.img" OMNI/NET.REQ >/dev/null 2>&1; then
  fail "the request was not consumed"
fi
echo "request: PASS (consumed, DHCP lease 10.0.2.15/24 via 10.0.2.2)"

# 3. Network recovery: the request pins the image's SHA-256; the loader downloads it over the
#    firmware's HTTP stack from the host (10.0.2.2 in QEMU user networking), verifies, starts it.
( cd "$B" && clang --target=x86_64-pc-windows-msvc -ffreestanding -nostdlib -fno-stack-protector \
    -mno-red-zone -O2 -Wall -Werror -c "$ROOT/tools/boot/external-recovery.c" -o ext.obj \
  && lld-link -subsystem:efi_application -entry:efi_main -nodefaultlib -machine:x64 -out:RECOVERY.EFI ext.obj ) \
  >/dev/null || fail "recovery image build"
HTTP_PORT=$(( 8100 + RANDOM % 800 ))
"$PY" -m http.server "$HTTP_PORT" --bind 127.0.0.1 --directory "$B" >"$B/http.log" 2>&1 &
HTTP_PID=$!
trap 'kill "$HTTP_PID" 2>/dev/null || true' EXIT
sleep 1
GOOD="$("$PY" -c "import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())" "$B/RECOVERY.EFI")"
recover() {
  printf 'dhcp\nrecover http://10.0.2.2:%s/RECOVERY.EFI sha256=%s\n' "$HTTP_PORT" "$1" > "$B/req"
  "$PY" tools/boot/fatimg.py put "$B/esp.img" OMNI/NET.REQ "$B/req"
  bash tools/boot/run-qemu.sh --no-build --esp "$B/esp.img"
}
recover "$GOOD"
need "AW_UEFI_NET_RECOVERY_FETCHED bytes=$(wc -c < "$B/RECOVERY.EFI")"
need "AW_UEFI_NET_RECOVERY_VERIFIED"
need "AW_EXTERNAL_RECOVERY_RAN"
need "AW_UEFI_NET_RECOVERY_RETURNED status=None"
echo "network recovery: PASS (downloaded, digest verified, started, returned)"

# 4. Same download, wrong pinned digest: refused before any byte of it runs.
recover "$(printf '%s' "$GOOD" | tr '0123456789abcdef' '123456789abcdef0')"
need "AW_UEFI_NET_RECOVERY_REFUSED reason=digest_mismatch"
if grep -qF AW_EXTERNAL_RECOVERY_RAN "$LOG"; then fail "an unverified image was started"; fi
echo "tampered download: PASS (digest mismatch, refused)"
echo "OMNI_OS_NETWORK=PASS"
