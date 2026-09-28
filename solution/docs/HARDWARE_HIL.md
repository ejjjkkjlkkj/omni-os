# Physical AMD hardware-in-the-loop

This workflow deliberately does not modify ASUS OEM firmware.

## Privileged trigger policy

Automatic hardware-in-the-loop execution is restricted to pushes on `main`. The Windows self-hosted stage runs selected validation under `NT AUTHORITY\\SYSTEM`, so development-branch pushes must not launch it implicitly. Testing another ref requires an explicit `workflow_dispatch` by an authorized operator.

Physical HIL remains separate from `SOFTWARE_CEILING_PASS`; a missing non-main HIL run cannot be reclassified as software evidence.

`Hardware Boundary Status` is informational on ordinary development-branch pushes. After a `Physical AMD HIL` run completes on `main` or any explicitly dispatched non-main ref, it is triggered from that completed run, checks the exact HIL head SHA, and enforces the fail-closed `hardware-boundary` CLI. Explicitly dispatched HIL runs are accepted as hardware evidence only for their exact commit; pull-request runs are never accepted as hardware evidence.

A hosted Linux runner builds one immutable OmniProbe.efi and three media forms:

- omni-fat.img for virtual boot
- omni-boot.vmdk for VMware Workstation
- omni-gpt.img with a GPT EFI System Partition for physical USB boot

The physical Windows self-hosted runner asserts the ASUS M1603QA / Ryzen 7 5800H identity, captures BIOS/board/Secure Boot/TPM/VBS/HDA inventory, then executes the same EFI payload under QEMU and VMware.

The UEFI payload is fail-closed. OMNI_UEFI_PASS is emitted only when:

- OMNI-CHALLENGE.TXT contains a valid 256-bit hexadecimal challenge;
- HII/IFR validation passes;
- OMNI-EVIDENCE.TXT is written and flushed successfully.

Virtual backends additionally require OMNI_CHALLENGE_PASS, OMNI_HII_PASS and OMNI_EVIDENCE_PASS.

## Final physical gate

Flash omni-gpt.img to removable media and mount it in Windows. Before reboot, verify that BOOTX64.EFI is the expected artifact and replace the CI challenge with a fresh challenge whose expected value is stored **off the boot media**:

    python tools/prepare_physical_media.py --mount D:\ --expected-sha256 <SHA256> --challenge-out C:\Temp\omni-expected-challenge.txt

The preparation tool:

- verifies EFI/BOOT/BOOTX64.EFI against the expected SHA-256;
- deletes any previous OMNI-EVIDENCE.TXT;
- generates a cryptographically random 256-bit challenge;
- writes OMNI-CHALLENGE.TXT to the boot media;
- stores the expected challenge in the requested off-media file.

Boot the USB through the real ASUS UEFI. After returning to Windows, read the same SMBIOS Type 1 UUID exposed by Windows and verify the fresh evidence:

    $platformUuid = (Get-CimInstance Win32_ComputerSystemProduct).UUID
    python tools/verify_uefi_evidence.py --evidence D:\OMNI-EVIDENCE.TXT --efi D:\EFI\BOOT\BOOTX64.EFI --expected-sha256 <SHA256> --expected-challenge <64_HEX_FROM_OFF_MEDIA_FILE> --expected-platform-uuid $platformUuid

OmniProbe records the SMBIOS Type 1 system UUID in pre-OS evidence when the platform exposes a meaningful UUID. When `--expected-platform-uuid` is supplied, the verifier requires an exact identity match.

A verifier PASS rejects:

- stale evidence from a previous boot;
- a missing, malformed or mismatched challenge;
- firmware-reported HII/UEFI failure;
- malformed or incomplete HII statistics;
- a modified EFI binary;
- a missing, malformed, zero/FF, or mismatched SMBIOS system UUID when physical identity binding is required.

This binds the returned evidence to the exact expected payload and to the fresh pre-boot challenge. It is still not a cryptographic remote attestation of the motherboard.

## Genuinely hardware-only remainder

After the physical evidence gate passes, software-only CI still cannot prove:

- speaker intelligibility and acoustic quality;
- physical HDA codec/amplifier/EAPD routing;
- real pre-OS keyboard behavior;
- TPM electrical/platform behavior and quotes;
- ASUS-specific firmware timing or other electrical/OEM behavior.

Those require observation on the actual machine.

## Per-run freshness binding

Virtual HIL evidence is bound to the exact Git commit **and GitHub workflow run ID**. The build derives a 64-hex challenge from `GITHUB_SHA:GITHUB_RUN_ID`, stores the binding in `omni-run-binding.json`, and embeds the challenge on the boot media. OmniProbe emits the exact challenge to serial/debug output and persists it in `OMNI-EVIDENCE.TXT`.

The Windows runner verifies the binding file against its current `GITHUB_SHA` and `GITHUB_RUN_ID` before launching privileged tests. QEMU and VMware run under PsExec SYSTEM, delete previous logs, and require the exact challenge plus all PASS markers and no FAIL markers. Evidence from another workflow run, even for the same commit, cannot satisfy the gate.
