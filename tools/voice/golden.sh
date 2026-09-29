#!/usr/bin/env bash
# Voice regression proof: synthesize a fixed 132-utterance corpus (FR/EN x male/female/child
# x modal/breathy/pressed/creaky, plus a rate/pitch sweep) with an `st` binary and require
# every WAV to be byte-identical to voice-st/tests/golden/corpus.sha256. The engine is
# deterministic, and toolchain-independent: MSVC, mingw-w64 and Linux builds must all match.
#   tools/voice/golden.sh PATH/TO/st[.exe]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ST="${1:?usage: golden.sh PATH/TO/st}"; W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
FR=("Bonjour, le menu de configuration est ouvert." "Heure: 14 h 30, adresse 192.168.1.10, version 2.5." "Le chercheur perd son ordinateur hier soir." "Haute herbe, hibou, huit hommes." "Appuyez sur Entrée pour valider, Échap pour quitter.")
EN=("Hello, the boot menu is ready." "Her brother answered the teacher over there." "The bird heard a word in the third year." "Press Enter to select, Escape to go back." "Secure boot is enabled; firmware version 1.2.3.")
say() { "$ST" --backend compact "$@" >/dev/null 2>&1; }
n=0
for v in male female child; do for q in modal breathy pressed creaky; do
  for t in "${FR[@]}"; do n=$((n+1)); say --voice $v --quality $q -l fr -t "$t" -o "$W/$n.wav"; done
  for t in "${EN[@]}"; do n=$((n+1)); say --voice $v --quality $q -l en -t "$t" -o "$W/$n.wav"; done
done; done
for r in 60 100 250; do for p in 90 220; do
  n=$((n+1)); say -l fr -r $r -p $p -t "${FR[1]}" -o "$W/$n.wav"
  n=$((n+1)); say -l en -r $r -p $p -t "${EN[1]}" -o "$W/$n.wav"
done; done
(cd "$W" && sha256sum *.wav | sed 's/ \*/  /' | sort -k2 -V) > "$W/got.sha256"
if diff -u "$ROOT/voice-st/tests/golden/corpus.sha256" "$W/got.sha256" > "$W/diff"; then
  echo "OMNI_OS_VOICE_GOLDEN=PASS utterances=$n"
else
  echo "voice output differs from the golden corpus:" >&2; head -20 "$W/diff" >&2; exit 1
fi
