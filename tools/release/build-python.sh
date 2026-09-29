#!/usr/bin/env bash
# The solution toolkit (omniexec-solution) as a wheel, and the whole repository as a source
# archive. Both are deterministic: timestamps come from the release commit.
#   tools/release/build-python.sh OUTDIR
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PY="$(command -v python || command -v python3)"
OUT="${1:?usage: build-python.sh OUTDIR}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct)"; export SOURCE_DATE_EPOCH
"$PY" -m pip wheel --quiet --no-deps --wheel-dir "$OUT" "$ROOT/solution"
git -C "$ROOT" archive --format=tar --prefix=omni-os/ HEAD | gzip -9 -n > "$OUT/omni-os-source.tar.gz"
