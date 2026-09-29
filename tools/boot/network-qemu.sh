#!/usr/bin/env bash
# Pre-boot network proofs (deny by default), in QEMU with its user-mode network:
#   1. no request  - interfaces are discovered read-only, no DHCP, nothing transmitted
#   2. request     - a one-shot \OMNI\NET.REQ left on the ESP is consumed before acting, then
#                    DHCP through the firmware stack yields QEMU's lease (10.0.2.15/24,
#                    gateway 10.0.2.2, DNS 10.0.2.3); the request is gone afterwards
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
echo "OMNI_OS_NETWORK=PASS"
