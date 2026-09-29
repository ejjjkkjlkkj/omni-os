#!/usr/bin/env bash
# OmniProbe.efi and OmniGuardianProbe.efi (solution/firmware/OmniPkg) with EDK II pinned to
# edk2-stable202608 (commit checked) and GCC; SOURCE_DATE_EPOCH is the EDK II commit time, so
# independent builds are byte-identical (solution's reproducibility gate).
#   tools/release/build-probe.sh OUTDIR
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:?usage: build-probe.sh OUTDIR}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
EDK2="$ROOT/build/edk2"; TAG=edk2-stable202608; SHA=2970e5699ba6267f3384ffab20f96647578aebc8
if [ ! -d "$EDK2/.git" ]; then
  git clone -q --depth 1 --branch "$TAG" --recurse-submodules https://github.com/tianocore/edk2.git "$EDK2"
fi
test "$(git -C "$EDK2" rev-parse HEAD)" = "$SHA"
make -s -C "$EDK2/BaseTools" >/dev/null
export WORKSPACE="$EDK2" PACKAGES_PATH="$EDK2:$ROOT/solution/firmware" PYTHON_COMMAND=python3
SOURCE_DATE_EPOCH="$(git -C "$EDK2" show -s --format=%ct "$SHA")"; export SOURCE_DATE_EPOCH
cd "$EDK2"
set +u  # edksetup.sh reads unset variables
# shellcheck disable=SC1091
source edksetup.sh BaseTools >/dev/null
build -a X64 -t GCC -b RELEASE -p "$ROOT/solution/firmware/OmniPkg/OmniPkg.dsc" >/dev/null
set -u
for app in OmniProbe OmniGuardianProbe; do
  bin="$(find Build/OmniPkg -type f -iname "$app.efi" | sort | head -1)"
  test -n "$bin"
  cp "$bin" "$OUT/$app.efi"
done
