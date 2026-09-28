#!/usr/bin/env bash
set -Eeuo pipefail

OPAM_REPOSITORY_SHA="6261f3c853ae417b354c06496e8458053131f14b"
OCAML_PACKAGE="ocaml-base-compiler.5.3.0"
FRAMAC_PACKAGE="frama-c.33.0"
ALT_ERGO_PACKAGE="alt-ergo.2.6.4"

FRAMAC_SOURCE_NAME="frama-c-33.0-Arsenic.tar.gz"
FRAMAC_SOURCE_SHA256="9c1cbffd28bb33c17a668107e39c96e4ae7378a3d8249f69b47afc7ee964e9b8"

FRAMAC_CACHE_URL="https://opam.ocaml.org/cache/sha256/9c/9c1cbffd28bb33c17a668107e39c96e4ae7378a3d8249f69b47afc7ee964e9b8"
FRAMAC_UPSTREAM_URL="https://www.frama-c.com/download/frama-c-33.0-Arsenic.tar.gz"

ROOT="${RUNNER_TEMP:-/tmp}/omni-opam-root"
REPO="${RUNNER_TEMP:-/tmp}/omni-opam-repository"
SOURCE_DIR="${RUNNER_TEMP:-/tmp}/omni-formal-sources"
FRAMAC_ARCHIVE="$SOURCE_DIR/$FRAMAC_SOURCE_NAME"

rm -rf "$ROOT" "$REPO" "$SOURCE_DIR"
mkdir -p "$SOURCE_DIR"

command -v curl >/dev/null
command -v sha256sum >/dev/null
command -v git >/dev/null
command -v opam >/dev/null
command -v python3 >/dev/null

SELECTED_SOURCE_URL=""

download_verified() {
    local url="$1"
    local tmp="$FRAMAC_ARCHIVE.part"
    local attempt
    local actual

    rm -f "$tmp"

    for attempt in 1 2 3; do
        echo "Downloading pinned Frama-C source:"
        echo "  $url"
        echo "  attempt $attempt/3"

        if curl \
            --fail \
            --location \
            --silent \
            --show-error \
            --connect-timeout 20 \
            --max-time 300 \
            --retry 2 \
            --retry-delay 2 \
            "$url" \
            --output "$tmp"
        then
            actual="$(sha256sum "$tmp" | awk '{print $1}')"

            if [[ "$actual" != "$FRAMAC_SOURCE_SHA256" ]]; then
                echo "SECURITY ERROR: Frama-C source SHA-256 mismatch."
                echo "Expected: $FRAMAC_SOURCE_SHA256"
                echo "Actual:   $actual"
                rm -f "$tmp"
                exit 1
            fi

            mv "$tmp" "$FRAMAC_ARCHIVE"
            SELECTED_SOURCE_URL="$url"

            echo "Frama-C archive SHA-256 PASS"
            return 0
        fi

        rm -f "$tmp"

        if [[ "$attempt" -lt 3 ]]; then
            sleep $((attempt * 5))
        fi
    done

    return 1
}

echo "=== FETCH FRAMA-C 33.0 ==="

if ! download_verified "$FRAMAC_CACHE_URL"; then
    echo "Official OPAM cache unavailable."
    echo "Trying the pinned upstream archive."

    if ! download_verified "$FRAMAC_UPSTREAM_URL"; then
        echo "ERROR: all pinned Frama-C source locations failed."
        exit 1
    fi
fi

echo "$FRAMAC_SOURCE_SHA256  $FRAMAC_ARCHIVE" | sha256sum -c -

echo "=== PIN OPAM REPOSITORY ==="

git clone https://github.com/ocaml/opam-repository.git "$REPO"
git -C "$REPO" checkout --detach "$OPAM_REPOSITORY_SHA"

test "$(git -C "$REPO" rev-parse HEAD)" = "$OPAM_REPOSITORY_SHA"

OPAM_FILE="$REPO/packages/frama-c/frama-c.33.0/opam"

test -f "$OPAM_FILE"

echo "=== REBIND FRAMA-C SOURCE TO VERIFIED LOCAL ARCHIVE ==="

python3 - "$OPAM_FILE" "$FRAMAC_ARCHIVE" "$FRAMAC_SOURCE_SHA256" "$FRAMAC_UPSTREAM_URL" <<'PY'
from pathlib import Path
import sys

opam_file = Path(sys.argv[1])
archive = Path(sys.argv[2]).resolve()
sha256 = sys.argv[3]
upstream = sys.argv[4]

text = opam_file.read_text(encoding="utf-8")

expected = f'''url {{
  src: "{upstream}"
  checksum: "sha256={sha256}"
}}'''

replacement = f'''url {{
  src: "{archive.as_uri()}"
  checksum: "sha256={sha256}"
}}'''

count = text.count(expected)

if count != 1:
    raise SystemExit(
        f"Expected exactly one locked Frama-C source block, found {count}"
    )

opam_file.write_text(
    text.replace(expected, replacement, 1),
    encoding="utf-8",
)

print("Pinned OPAM Frama-C metadata rebound to verified local archive")
PY

grep -F "sha256=$FRAMAC_SOURCE_SHA256" "$OPAM_FILE"

echo "=== CREATE CLEAN OPAM ENVIRONMENT ==="

export OPAMROOT="$ROOT"

opam init \
    --bare \
    --disable-sandboxing \
    --no-setup \
    -y \
    default \
    "file://$REPO"

opam switch create omni-formal "$OCAML_PACKAGE" -y

eval "$(opam env --switch=omni-formal --set-switch)"

echo "=== INSTALL EXACT PINNED TOOLCHAIN ==="

installed=0

for attempt in 1 2 3 4; do
    if opam install -y "$FRAMAC_PACKAGE" "$ALT_ERGO_PACKAGE"; then
        installed=1
        break
    fi

    if [[ "$attempt" -lt 4 ]]; then
        echo "Pinned OPAM install attempt $attempt failed."
        echo "Retrying exact same locked inputs."
        sleep $((attempt * 10))
    fi
done

if [[ "$installed" -ne 1 ]]; then
    echo "ERROR: pinned Frama-C/Alt-Ergo installation failed."
    exit 1
fi

echo "=== VERIFY INSTALLED VERSIONS ==="

frama-c -version | grep -F "33.0 (Arsenic)"
alt-ergo --version

echo "=== EXPORT REPRODUCIBILITY EVIDENCE ==="

WORKSPACE="${GITHUB_WORKSPACE:-$PWD}"

opam switch export "$WORKSPACE/formal-toolchain.opam.export"

opam list \
    --installed \
    --columns=name,version \
    > "$WORKSPACE/formal-toolchain-packages.txt"

cat > "$WORKSPACE/formal-toolchain-pins.txt" <<EOF
OPAM_REPOSITORY_SHA=$OPAM_REPOSITORY_SHA
OCAML_PACKAGE=$OCAML_PACKAGE
FRAMAC_PACKAGE=$FRAMAC_PACKAGE
ALT_ERGO_PACKAGE=$ALT_ERGO_PACKAGE
FRAMAC_SOURCE_SHA256=$FRAMAC_SOURCE_SHA256
FRAMAC_SOURCE_URL=$SELECTED_SOURCE_URL
EOF

if [[ -n "${GITHUB_ENV:-}" ]]; then
    printf 'OPAMROOT=%s\nOPAMSWITCH=omni-formal\n' \
        "$ROOT" >> "$GITHUB_ENV"
fi

if [[ -n "${GITHUB_PATH:-}" ]]; then
    opam var bin --switch=omni-formal >> "$GITHUB_PATH"
fi

echo "Frama-C pinned installation PASS"
echo "export OPAMROOT=$ROOT"
echo 'eval "$(opam env --switch=omni-formal --set-switch)"'
