#!/usr/bin/env bash
# Checks an omni-os QEMU boot log (debugcon output). Used by CI and by run-qemu.sh.
#   tools/boot/check-log.sh boot.log      -> prints OMNI_OS_BOOT=PASS or fails
set -euo pipefail
LOG="${1:?usage: check-log.sh <boot.log>}"
need() { grep -qF "$1" "$LOG" || { echo "missing $1" >&2; exit 1; }; }

for m in AW_BOOT_OK AW_ACPI_VALIDATE_OK AW_UEFI_SR_PROOF_OK AW_EXIT_BOOT_SERVICES_OK \
         AW_MEMORY_MAP_HANDOFF_OK AW_NATIVE_KERNEL_ENTRY_OK AW_PAGING_BASELINE_OK \
         AW_PCI_NVME_FOUND AW_PCI_XHCI_FOUND AW_PCI_HDA_FOUND AW_SCHED_PROOF_OK \
         AW_NATIVE_FRAMEBUFFER_WRITE_OK AW_NATIVE_KERNEL_IDLE; do
  need "$m"
done

# Speech really plays (run-qemu.sh attaches an HDA codec): the loader's screen reader
# and the kernel both stream speech PCM by DMA.
need "AW_UEFI_AUDIO_BACKEND channel=hda"
for m in AW_UEFI_HDA_READY AW_HDA_PLAYBACK_PROOF_OK AW_HDA_SPEECH_PROOF_OK; do need "$m"; done

# Network, deny by default: interfaces are discovered read-only and nothing is transmitted.
# Native Recovery Core: the booted generation was selected by the boot-state rules (its digest
# is verified on every boot after the first; tools/boot/recovery-qemu.sh proves the rest).
need "AW_RECOVERY_BOOT generation="
need "AW_UEFI_NET nics="
need "AW_UEFI_MEASURED tpm="
need "policy=deny-by-default transmitted=0"

# Timer-driven proofs need x2APIC, which older QEMU TCG CPU models omit:
# then the kernel must skip them cleanly, never start them.
if grep -qF AW_APIC_TIMER_UNAVAILABLE "$LOG"; then
  need AW_PREEMPT_SKIPPED; need AW_RING3_PREEMPT_SKIPPED; need AW_SMP_UNAVAILABLE
  if grep -qF AW_PREEMPT_BEGIN "$LOG"; then
    echo "preemption started without a proven timer" >&2; exit 1
  fi
else
  need AW_APIC_TIMER_DELIVERY_PROOF_OK; need AW_PREEMPT_PROOF_OK; need AW_RING3_PREEMPT_PROOF_OK
fi
if grep -qF AW_CPU_X2APIC_AVAILABLE "$LOG"; then need AW_MADT_OK; need AW_IOAPIC_DELIVERY_PROOF_OK; fi

# `! grep` would be ignored by `set -e`: fail explicitly on any failure marker.
if grep -E 'AW_NATIVE_KERNEL_PANIC|AW_NATIVE_EXCEPTION|AW_IOAPIC_IRQ_NOT_FIRED|AW_IOAPIC_MASK_INEFFECTIVE|AW_IOAPIC_DID_NOT_RESUME' "$LOG"; then
  echo "failure marker in boot log" >&2; exit 1
fi
echo "OMNI_OS_BOOT=PASS markers=$(grep -oE 'AW_[A-Z0-9_]+' "$LOG" | sort -u | wc -l)"
