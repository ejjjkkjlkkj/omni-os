#!/usr/bin/env bash
# Boots OmniProbe.efi (solution/firmware/OmniPkg) in QEMU/OVMF and checks the proofs it must
# produce, as solution's own CI does: the challenge written on the ESP is echoed, HII and UEFI
# checks pass, the platform UUID is read, and OMNI-EVIDENCE.TXT is written back to the disk.
# Linux (dosfstools + mtools: the probe writes a long-name file, read back with mcopy).
#   tools/boot/probe-qemu.sh OmniProbe.efi    -> OMNI_OS_PROBE=PASS, OMNI_OS_GUARDIAN=PASS or fails
set -euo pipefail
EFI="$(cd "$(dirname "${1:?usage: probe-qemu.sh OmniProbe.efi}")" && pwd)/$(basename "$1")"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"; W="$ROOT/build/probe-proof"; rm -rf "$W"; mkdir -p "$W"; cd "$W"
fail() { echo "PROBE PROOF FAILED: $*" >&2; exit 1; }
UUID=12345678-1234-5678-9abc-def012345678
CODE=/usr/share/OVMF/OVMF_CODE_4M.fd; VARS=/usr/share/OVMF/OVMF_VARS_4M.fd
[ -f "$CODE" ] || { CODE=/usr/share/OVMF/OVMF_CODE.fd; VARS=/usr/share/OVMF/OVMF_VARS.fd; }
cp "$VARS" vars.fd
truncate -s 64M esp.img && mkfs.vfat -n OMNIPROBE esp.img >/dev/null
mmd -i esp.img ::/EFI ::/EFI/BOOT
mcopy -i esp.img "$EFI" ::/EFI/BOOT/BOOTX64.EFI
printf 'omni-os-release-probe' | sha256sum | awk '{print $1}' > challenge.txt
mcopy -i esp.img challenge.txt ::/OMNI-CHALLENGE.TXT
set +e
timeout 60s qemu-system-x86_64 -machine q35,accel=tcg -m 256 -uuid "$UUID" -display none \
  -serial none -net none -no-reboot -debugcon file:debug.log -global isa-debugcon.iobase=0x402 \
  -drive if=pflash,format=raw,readonly=on,file="$CODE" -drive if=pflash,format=raw,file=vars.fd \
  -drive file=esp.img,format=raw
rc=$?
set -e
for m in OMNI_CHALLENGE_PASS OMNI_HII_PASS OMNI_EVIDENCE_PASS OMNI_UEFI_PASS "OMNI_PLATFORM_UUID=$UUID"; do
  grep -aqF "$m" debug.log || { tail -20 debug.log >&2; fail "missing $m"; }
done
# Pinned QEMU powers off when the probe resets; the distribution QEMU may not, so a timeout is
# accepted only once every proof marker is present (the evidence file is checked below too).
case "$rc" in 0|124) ;; *) fail "QEMU exited $rc" ;; esac
mcopy -i esp.img ::OMNI-EVIDENCE.TXT evidence.txt || fail "no OMNI-EVIDENCE.TXT"
for m in OMNI_EVIDENCE_V2 "OMNI_CHALLENGE=$(cat challenge.txt)" OMNI_HII_PASS OMNI_UEFI_PASS "OMNI_PLATFORM_UUID=$UUID"; do
  grep -aqF "$m" evidence.txt || fail "evidence file lacks $m"
done
echo "OMNI_OS_PROBE=PASS"

# OmniGuardianProbe (next to OmniProbe.efi): read-only inventory of the firmware's security and
# network capabilities, written to OMNI-GUARDIAN.TXT. It returns to the firmware instead of
# powering off, so the run is bounded and the file is read back afterwards.
GUARD="$(dirname "$EFI")/OmniGuardianProbe.efi"
[ -f "$GUARD" ] || fail "no $GUARD"
truncate -s 64M guard.img && mkfs.vfat -n OMNIGUARD guard.img >/dev/null
mmd -i guard.img ::/EFI ::/EFI/BOOT
mcopy -i guard.img "$GUARD" ::/EFI/BOOT/BOOTX64.EFI
cp "$VARS" vars.fd
timeout 40s qemu-system-x86_64 -machine q35,accel=tcg -m 256 -display none -serial none -net none \
  -drive if=pflash,format=raw,readonly=on,file="$CODE" -drive if=pflash,format=raw,file=vars.fd \
  -drive file=guard.img,format=raw || true
mcopy -i guard.img ::OMNI-GUARDIAN.TXT guardian.txt || fail "no OMNI-GUARDIAN.TXT"
for m in OMNI_GUARDIAN_V1 SCHEMA=omni.guardian.capabilities.v1 MODE=READ_ONLY_CAPABILITY_DISCOVERY \
         SECURE_BOOT_PRESENT= SNP_HANDLES= TCG2_HANDLES= VERDICT=CAPABILITY_INVENTORY_COMPLETE; do
  grep -aqF "$m" guardian.txt || fail "guardian inventory lacks $m"
done
echo "OMNI_OS_GUARDIAN=PASS"
