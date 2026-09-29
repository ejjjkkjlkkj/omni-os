#!/usr/bin/env bash
# Builds NAVIGATION.EFI (spoken BIOS navigation), boots it in QEMU with an HDA codec,
# drives F1/Down/Up/Escape over QMP and verifies the serial proof and the captured PCM.
# Linux only (QMP over a unix socket). Needs: clang, lld, qemu-system-x86_64, OVMF, python3.
#   tools/boot/navigation-qemu.sh          -> OMNI_OS_NAVIGATION_BOOT=PASS or fails
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NAV="$ROOT/navigation"; UEFI="$NAV/navigation/uefi"; B="$ROOT/build/navigation"
rm -rf "$B"; mkdir -p "$B"
cd "$NAV"  # the generators resolve their data relative to the component root

# 1. Build the UEFI application (shared with releases) and its removable media.
"$ROOT/tools/boot/build-navigation.sh" "$B"
python3 "$UEFI/create_fat12_boot_image.py" "$B/NAVIGATION.EFI" "$B/uefi-floppy.img"
test -s "$B/uefi-floppy.img"

# 2. Firmware.
OVMF_CODE=/usr/share/OVMF/OVMF_CODE_4M.fd; OVMF_VARS=/usr/share/OVMF/OVMF_VARS_4M.fd
[ -f "$OVMF_CODE" ] || { OVMF_CODE=/usr/share/OVMF/OVMF_CODE.fd; OVMF_VARS=/usr/share/OVMF/OVMF_VARS.fd; }
test -f "$OVMF_CODE" && test -f "$OVMF_VARS"

# boot <tag> <extra smoke args...>: one QEMU run driven by qmp_navigation_smoke.py
boot() {
  local tag="$1"; shift
  cp "$OVMF_VARS" "$B/vars-$tag.fd"
  qemu-system-x86_64 -machine q35,accel=tcg -m 512M -smp 1 -nodefaults -no-reboot \
    -display none -monitor none \
    -qmp unix:"$B/qmp-$tag.sock",server=on,wait=off \
    -serial file:"$B/serial-$tag.log" \
    -debugcon file:"$B/ovmf-$tag.log" -global isa-debugcon.iobase=0x402 \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$B/vars-$tag.fd" \
    -device qemu-xhci,id=xhci \
    -drive if=none,id=bootstick,format=raw,file="$B/uefi-floppy.img",readonly=on \
    -device usb-storage,bus=xhci.0,drive=bootstick,bootindex=1 \
    -device ich9-intel-hda,id=hda0 \
    -audiodev wav,id=wav0,path="$B/audio-$tag.wav",out.frequency=48000,out.channels=2,out.format=s16 \
    -device hda-output,bus=hda0.0,audiodev=wav0 \
    >"$B/qemu-$tag.out" 2>"$B/qemu-$tag.err" &
  local pid=$!
  # shellcheck disable=SC2064
  trap "kill $pid 2>/dev/null || true" EXIT
  python3 "$UEFI/qmp_navigation_smoke.py" --qmp "$B/qmp-$tag.sock" --serial "$B/serial-$tag.log" \
    --report "$B/navigation-$tag.json" --timeout 30 "$@" | tee "$B/navigation-$tag.txt"
  for _ in $(seq 1 100); do kill -0 "$pid" 2>/dev/null || break; sleep 0.05; done
  kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true
  trap - EXIT
}
need() { grep -qF "$1" "$2" || { echo "missing $1 in $(basename "$2")" >&2; exit 1; }; }

# 3. Interruptible run: navigation keys interrupt speech in real time.
boot live --delay 0.75
S="$B/serial-live.log"
for m in QEVARYNOX-UEFI-HII-GRAPH-PROMPT-SPEECH-V1 STATE=START HDA_CONTROLLER_CODEC=PASS \
         HDA_GRAPH_SEARCH_LIVE=PASS HDA_OUTPUT_PATH_CONFIGURATION=PASS HII_GRAPH_NAV_DISCOVERY_PROMPT=PASS \
         HII_GRAPH_SPEECH_DMA=PASS HII_PROMPT_SPEECH_HDA=PASS LPIB_PROGRESS=PASS \
         HII_GRAPH_NAV_KEY=F1 HII_GRAPH_NAV_HELP_DISCOVERABLE=PASS HII_GRAPH_NAV_KEY=DOWN \
         HII_GRAPH_NAV_KEY=UP HII_GRAPH_NAV_KEY=ESC HII_GRAPH_NAV_SIMPLE_EXIT=PASS \
         HII_GRAPH_NAV_EXIT=PASS HII_GRAPH_NAV_SPEECH_INTERRUPT=PASS; do
  need "$m" "$S"
done
realtime="$(grep -cF 'HII_GRAPH_NAV_REALTIME_FOCUS_SPEECH=PASS' "$S" || true)"
[ "$realtime" -ge 3 ] || { echo "realtime focus speech: $realtime < 3" >&2; exit 1; }
python3 "$UEFI/analyze_wav_evidence.py" "$B/audio-live.wav" "$B/audio-live.json"
python3 "$UEFI/verify_speech_pcm.py" "$B/audio-live.wav" "$B/speech-live.json" | tee "$B/speech-live.txt"
need DISCOVERY_SPEECH_CONTENT=PASS "$B/speech-live.txt"
need DISCOVERY_SPEECH_PCM_EXACT=PASS "$B/speech-live.txt"

# 4. Complete run: every phrase is spoken to the end, and the PCM is bit-identical.
boot complete --wait-speech-complete --speech-timeout 120 --delay 0.50
T="$B/navigation-complete.txt"
for m in NAVIGATION_COMPLETE_SPEECH_SCENARIO=PASS QEMU_SPEECH_F1_COMPLETE=PASS \
         QEMU_SPEECH_DOWN_COMPLETE=PASS QEMU_SPEECH_UP_COMPLETE=PASS; do
  need "$m" "$T"
done
python3 "$UEFI/analyze_wav_evidence.py" "$B/audio-complete.wav" "$B/audio-complete.json"
python3 "$UEFI/verify_navigation_speech_pcm.py" "$B/audio-complete.wav" "$B/serial-complete.log" \
  "$B/speech-complete.json" | tee "$B/speech-complete.txt"
for m in NAVIGATION_SPEECH_PCM_FORMAT=PASS NAVIGATION_DISCOVERY_PREFIX=PASS FULL_NAVIGATION_SPEECH_CONTENT=PASS; do
  need "$m" "$B/speech-complete.txt"
done
echo "OMNI_OS_NAVIGATION_BOOT=PASS"
