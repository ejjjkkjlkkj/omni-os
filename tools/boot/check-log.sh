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
need "AW_UEFI_INVENTORY known=272 present="
# Every protocol the firmware installs is used for real (tools: os/boot/uefi/src/protocols.rs):
# a wrong answer or a present protocol without a use fails the boot proof.
need "AW_UEFI_PROTOCOLS present="
# UEFI runtime services reach the kernel (Memory Attributes Table or firmware tables per call),
# and the kernel's seven runtime-health checks all pass on every boot (a trial generation is
# promoted only with them: tools/boot/recovery-qemu.sh).
need "AW_UEFI_RUNTIME_HANDOFF present=true"
# The kernel drives the NVMe controller itself: IDENTIFY, I/O queues and a DMA read of LBA 0.
need "AW_NVME_PROOF_OK"
# Native accessibility: the administration session navigated and voiced, and the OS renders
# arbitrary text to real speech with its own synthesizer.
need "AW_ADMIN_PROOF_OK panels=6"
need "AW_OS_TTS_PROOF_OK"
need "AW_NVME_READ_OK lba=0"
need "AW_UEFI_RUNTIME_READY mode="
need "AW_HEALTH_CHECKS kernel=pass storage=pass input=pass audio=pass accessibility=pass speech=pass security=pass"
grep -aqE "AW_UEFI_PROTOCOLS .* failed=0 unclassified=0" "$LOG"   || { grep -a "AW_UEFI_PROTOCOL" "$LOG" | grep -aE "use=(failed|unclassified)" >&2 || true
       echo "a firmware protocol failed or has no use" >&2; exit 1; }
need "AW_UEFI_PLATFORM_RNG present="
need "AW_UEFI_PLATFORM_ESRT present="
need "AW_UEFI_PLATFORM_RECOVERY platform_options="
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
