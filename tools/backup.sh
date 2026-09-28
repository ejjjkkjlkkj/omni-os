#!/usr/bin/env bash
# Availability: full offline backup of omni-os (all branches, all history).
#   tools/backup.sh [dest-dir]      default dest: /c/OMNI-BACKUPS/omni-os
# Restore:   git clone <file>.bundle omni-os && cd omni-os && git fetch origin '+refs/heads/*:refs/heads/*'
set -euo pipefail
cd "$(dirname "$0")/.."
dest="${1:-/c/OMNI-BACKUPS/omni-os}"
mkdir -p "$dest"
git fetch --quiet origin '+refs/heads/*:refs/remotes/origin/*'
name="omni-os-$(date +%Y%m%d-%H%M%S)-$(git rev-parse --short origin/main)"
git bundle create --quiet "$dest/$name.bundle" --branches --remotes=origin
git bundle verify --quiet "$dest/$name.bundle"
(cd "$dest" && sha256sum "$name.bundle" > "$name.bundle.sha256")
echo "BACKUP=OK $dest/$name.bundle ($(git bundle list-heads "$dest/$name.bundle" | wc -l) refs)"
