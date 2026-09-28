#Requires -Version 7.0
<#
.SYNOPSIS
    Bare-metal anti-regression suite: boots every kernel configuration under
    QEMU/OVMF and asserts the markers each one must produce.

.DESCRIPTION
    Section 1.1 of the project dossier: a component is never PASS because it
    compiled. Every claim below is tied to a marker the kernel only emits after
    the CPU actually did the thing - delivered an interrupt, refused a write,
    refused an instruction fetch, entered #DF on its IST stack.

    Forbidden markers matter as much as required ones: a boot that reaches
    AW_NATIVE_KERNEL_IDLE while also printing AW_MEMORY_PROTECTION_FAIL is a
    failure, not a pass.
#>
[CmdletBinding()]
param(
    [string]$Qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe',
    [ValidateRange(5, 300)][int]$TimeoutSeconds = 90,
    # Optional: run only the named configurations (e.g. -Only fat-write). Staging
    # still builds every disk image; only the boot runs are filtered. Empty = all.
    [string[]]$Only = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$boot = Join-Path $PSScriptRoot 'Invoke-KernelBoot.ps1'

# A FAT16 data disk carrying HELLO.TXT, for the virtio-blk and filesystem
# proofs. Built with the same pure-Python imager as the bootable image.
$repoRoot = Split-Path $PSScriptRoot -Parent
$vblkDisk = Join-Path $repoRoot 'target/virtio-test.img'
$fatStage = Join-Path $repoRoot 'target/fat-stage'
if (Test-Path -LiteralPath $fatStage) { Remove-Item -LiteralPath $fatStage -Recurse -Force }
New-Item -ItemType Directory -Path $fatStage -Force | Out-Null
[System.IO.File]::WriteAllBytes(
    (Join-Path $fatStage 'HELLO.TXT'),
    [System.Text.Encoding]::ASCII.GetBytes("ACCESSIBLE-WINDOWS-FS-OK`n"))
$python = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction Stop)
# A hand-built userland ELF (USERPROG.ELF -> 8.3 "USERPROGELF"), for the loader
# proof: the kernel reads it off this same disk and runs it at CPL3.
& $python.Source (Join-Path $PSScriptRoot 'make-user-elf.py') (Join-Path $fatStage 'USERPROG.ELF')
if ($LASTEXITCODE -ne 0) { throw 'building the userland test ELF failed' }
# Two spinner programs at distinct bases (USERA.ELF/USERB.ELF), for the init proof
# that preemptively schedules two userland programs at once.
& $python.Source (Join-Path $PSScriptRoot 'make-user-elf.py') (Join-Path $fatStage 'USERA.ELF') '--spinner' '--base' '0x500000000'
if ($LASTEXITCODE -ne 0) { throw 'building userland spinner A failed' }
& $python.Source (Join-Path $PSScriptRoot 'make-user-elf.py') (Join-Path $fatStage 'USERB.ELF') '--spinner' '--base' '0x600000000'
if ($LASTEXITCODE -ne 0) { throw 'building userland spinner B failed' }
# Two IPC programs (USERIPCA/USERIPCB) for the channel + handle-model proof.
& $python.Source (Join-Path $PSScriptRoot 'make-user-elf.py') (Join-Path $fatStage 'USERIPCA.ELF') '--ipc-sender' '--base' '0x700000000'
if ($LASTEXITCODE -ne 0) { throw 'building the IPC sender failed' }
& $python.Source (Join-Path $PSScriptRoot 'make-user-elf.py') (Join-Path $fatStage 'USERIPCB.ELF') '--ipc-receiver' '--base' '0x800000000'
if ($LASTEXITCODE -ne 0) { throw 'building the IPC receiver failed' }
& $python.Source (Join-Path $PSScriptRoot 'build_bootable_image.py') '--fat-only' $vblkDisk $fatStage
if ($LASTEXITCODE -ne 0) { throw 'building the FAT16 test disk failed' }
# A full GPT disk (protective MBR + GPT + FAT16 ESP) for the GPT parser proof.
$gptDisk = Join-Path $repoRoot 'target/gpt-test.img'
& $python.Source (Join-Path $PSScriptRoot 'build_bootable_image.py') $gptDisk $fatStage
if ($LASTEXITCODE -ne 0) { throw 'building the GPT test disk failed' }

# A blank scratch disk for the AHCI write proof: it overwrites LBA 0, so it must
# never be a data disk. Recreated blank each run.
$ahciScratch = Join-Path $repoRoot 'target/ahci-scratch.img'
# A blank scratch disk for the NVMe write proof (it overwrites LBA 2).
$nvmeScratch = Join-Path $repoRoot 'target/nvme-scratch.img'
[System.IO.File]::WriteAllBytes($nvmeScratch, (New-Object byte[] (1024 * 1024)))
[System.IO.File]::WriteAllBytes($ahciScratch, (New-Object byte[] (1024 * 1024)))

# A dedicated FAT16 scratch disk for the filesystem-write proof: a fresh, valid
# FAT16 volume (so it has free clusters and a free root slot). The proof creates a
# file on it, so it must be its own disk, recreated each run - never a data disk.
$fatScratch = Join-Path $repoRoot 'target/fat-write-scratch.img'
& $python.Source (Join-Path $PSScriptRoot 'build_bootable_image.py') '--fat-only' $fatScratch $fatStage
if ($LASTEXITCODE -ne 0) { throw 'building the FAT16 write-scratch disk failed' }

# A blank 8 MiB (16384-sector) scratch disk for the GPT-write proof: it writes a
# whole partition table, so it must be its own blank disk, recreated each run. The
# size must match the sector count the kernel passes to gpt::prove_write (16384).
$gptScratch = Join-Path $repoRoot 'target/gpt-write-scratch.img'
[System.IO.File]::WriteAllBytes($gptScratch, (New-Object byte[] (16384 * 512)))

# A blank 8 MiB (16384-sector) scratch disk for the FAT16 format (mkfs) proof: it
# writes a fresh filesystem over the whole disk, so it must be its own blank disk,
# recreated each run. The size must match what the kernel passes to prove_format.
$mkfsScratch = Join-Path $repoRoot 'target/mkfs-scratch.img'
[System.IO.File]::WriteAllBytes($mkfsScratch, (New-Object byte[] (16384 * 512)))

# A blank 8 MiB (16384-sector) scratch disk for the disk-build capstone: the kernel
# partitions and formats the whole disk, so it must be its own blank disk, recreated
# each run. The size must match what the kernel passes to installer::prove (16384).
$buildScratch = Join-Path $repoRoot 'target/disk-build-scratch.img'
[System.IO.File]::WriteAllBytes($buildScratch, (New-Object byte[] (16384 * 512)))

# Where the serial config routes COM1, so its banner can be checked host-side.
$serialFile = Join-Path $repoRoot 'target/serial-com1.log'
if (Test-Path -LiteralPath $serialFile) { Remove-Item -LiteralPath $serialFile -Force }

$configurations = @(
    @{
        Name     = 'normal'
        Features = @()
        Required = @(
            # UEFI loader stage.
            'AW_BOOT_OK stage=uefi_init arch=x86_64'
            # Accessibility begins at the firmware stage, before the kernel loads:
            # the same announcement engine the kernel and installer use voices the
            # boot screen on the visible UEFI console (and the debug console it is
            # asserted on here), states the real display mode, then waits for a
            # keyboard review before continuing - operable, nonvisual delivery
            # evidence that a user is told the machine is coming up, in words, this
            # early. The display line varies with the firmware's mode, so it is not
            # pinned; the constant lines are. With no key pressed (as here) the
            # review window times out and the boot continues on its own.
            'AW_UEFI_SR_BEGIN'
            # The first audible layer: a startup chime through the PC speaker
            # (PIT channel 2 + port 0x61), so a blind user hears the accessible
            # boot come up before any text. Actual sound is not captured headless,
            # but the speaker interface being driven is proved by reading port
            # 0x61 back as gated then silenced. A machine with no beeper is silent
            # yet still passes; spoken words await the HDA + synthesis work.
            'AW_UEFI_SND_BEGIN'
            'AW_UEFI_SND_TONE freq=660'
            'AW_UEFI_SND_GATED'
            'AW_UEFI_SND_TONE freq=990'
            'AW_UEFI_SND_SILENCED'
            'AW_UEFI_SND_PROOF_OK'
            'AW_UEFI_SR_SPEAK "Accessible Windows, window"'
            'AW_UEFI_SR_SPEAK "Screen reader active at firmware stage"'
            'AW_UEFI_SR_SPEAK "Starting Accessible Windows"'
            'AW_UEFI_SR_SPEAK "Loading the operating system"'
            'AW_UEFI_SR_READY'
            # The firmware-stage accessible Setup Utility (AMI/ASUS-style tabs: Main,
            # Advanced, Boot, Security, Save and Exit), operated on the firmware's own
            # keyboard (so a USB keyboard works on every machine, before any kernel USB
            # stack) and voiced through HDA. Real boot entries are enumerated from the
            # firmware's BootOrder/Boot#### variables; the Boot tab opens on the safe
            # default. The setup defaults to French (a Language item switches to English),
            # so the default item is "Demarrer normalement"; the tab title is "Demarrage".
            # The engine's role words ("tab", "menu item") stay English in the markers. The
            # 1-of count varies with the number of boot entries, so assert only the prefix.
            'AW_UEFI_SETUP_TAB "Démarrage, tab,'
            'AW_UEFI_BOOT_ENUM count='
            'AW_UEFI_MENU_ITEM "Démarrer normalement, menu item, 1 of'
            # With no key pressed the countdown boots normally on its own.
            'AW_UEFI_SR_CONTINUE reason=timeout'
            'AW_UEFI_SR_PROOF_OK'
            'AW_KERNEL_FILE_READ_OK'
            'AW_KERNEL_IMAGE_HEADER_OK base=0x200000'
            'AW_NATIVE_KERNEL_LOAD_OK address=0x200000'
            'mode=fixed_base'
            'AW_MEMORY_MAP_OK'
            'AW_ACPI_OK'
            'AW_ACPI_VALIDATE_OK'
            'AW_GOP_OK'
            'AW_FRAMEBUFFER_OK'
            'AW_EXIT_BOOT_SERVICES_OK'
            'AW_MEMORY_MAP_HANDOFF_OK'
            'AW_KERNEL_HANDOFF_OK'
            'AW_NATIVE_KERNEL_TRANSFER address=0x200000 entry=0x201000'
            # Fixed-base image load and segment reload.
            'AW_GDT_IDT_BEGIN'
            'AW_GDT_LOADED'
            'AW_GDT_SEGMENTS_RELOADED cs=0x0000000000000008 ss=0x0000000000000010'
            'AW_IDT_LOADED'
            'AW_TSS_IST_READY vector=8 ist=1'
            'AW_IDT_VECTOR_COUNT 32/256'
            'AW_NATIVE_KERNEL_ENTRY_OK'
            # Kernel-owned W^X page tables.
            'AW_VMM_IDENTITY_MAP_OK'
            'AW_VMM_ACTIVE'
            # One persistent frame owner, then a real runtime map/unmap on the
            # live tables: a store reaches the frame through the new address and
            # its identity address, translation agrees, and after unmap the
            # address faults not-present.
            'AW_FRAME_ALLOCATOR_OK'
            'AW_VMM_MAP_OK va=0x0000000100000000'
            'AW_VMM_MAP_READBACK_OK'
            'AW_VMM_MAP_TRANSLATE_OK'
            'AW_VMM_UNMAP_OK'
            'AW_VMM_UNMAP_FAULT_OK'
            'AW_VMM_RUNTIME_MAP_PROOF_OK'
            # A kernel heap behind the global allocator: Box/Vec allocate, a
            # vector grows and sums, a freed block is reused, and an over-aligned
            # allocation comes back aligned.
            'AW_HEAP_MAP_OK pages=512'
            'AW_HEAP_BOX_OK'
            'AW_HEAP_VEC_OK sum=499500'
            'AW_HEAP_REUSE_OK'
            'AW_HEAP_ALIGN_OK'
            'AW_HEAP_PROOF_OK'
            # A real drop to Ring 3 and back: the user routine ran at CPL3 and
            # made a syscall carrying a known number and argument, and its return
            # address lands inside the user code page - it came from CPL3, not
            # from anywhere in the kernel.
            'AW_RING3_MAP_OK code_va=0x0000000200000000'
            # A versioned syscall ABI over sysret: add returns 5, and a validated
            # user pointer is copied into the kernel (SMAP-guarded) - 13 bytes.
            'AW_SYSCALL_ADD result=5'
            'AW_SYSCALL_WRITE copied=13'
            'AW_RING3_PROOF_OK'
            'AW_SYSCALL_ABI_PROOF_OK version=1'
            # The syscall entry ran on a user GS base and had to swapgs to reach
            # the kernel per-CPU block: gs:[0] matched this CPU's real per-CPU base.
            'AW_SWAPGS_PROOF_OK'
            # Cooperative round-robin scheduler: three kernel threads context
            # switch and take exactly ten turns each over thirty yields.
            'AW_SCHED_THREAD id=0 count=10'
            'AW_SCHED_THREAD id=1 count=10'
            'AW_SCHED_THREAD id=2 count=10'
            'AW_SCHED_PROOF_OK threads=3 switches=29'
            # Preemptive scheduling: three threads that never yield are still
            # switched, driven only by timer interrupts, in exactly twelve
            # timer-driven context switches.
            'AW_PREEMPT_PROOF_OK threads=3 switches=12'
            # Ring 3 preemption: a CPL3 user thread that never makes a syscall is
            # interrupted by the timer and descheduled by the kernel, with swapgs
            # keeping the per-CPU GS correct across the privilege boundary.
            'AW_RING3_PREEMPT_PROOF_OK'
            # CPU protection bits actually latched.
            'AW_SECURITY_ENFORCED wp=1 nx=1'
            'AW_SECURITY_BASELINE_OK'
            # Protections proved by real faults, with the right error codes.
            'AW_MEMORY_PROTECTION_OK name=nx-execute-rodata error_code=0x0000000000000011'
            'AW_MEMORY_PROTECTION_OK name=nx-execute-data error_code=0x0000000000000011'
            'AW_MEMORY_PROTECTION_OK name=wx-write-text error_code=0x0000000000000003'
            'AW_MEMORY_PROTECTION_OK name=guard-page error_code=0x0000000000000000'
            'AW_MEMORY_PROTECTION_PROOF_OK'
            # Real APIC timer delivery, monotonic, and stopped by masking.
            'AW_APIC_TIMER_ARMED mode=periodic'
            'AW_APIC_TIMER_FIRED'
            'AW_APIC_TIMER_MONOTONIC_OK'
            'AW_APIC_TIMER_MASKED_STOPPED'
            'AW_APIC_TIMER_UNMASKED_RESUMED'
            'AW_APIC_TIMER_DELIVERY_PROOF_OK'
            # A real device interrupt, routed by the MADT through an I/O APIC.
            # gsi=2 for isa_irq=0 is the point: the interrupt source override
            # was applied, not the IRQ number assumed to be the GSI number.
            'AW_MADT_OK'
            'AW_IOAPIC_FOUND id=0 madt_id=0 base=0x00000000fec00000 entries=24'
            'AW_IOAPIC_ROUTED isa_irq=0 gsi=2 index=2 vector=0x0000000000000050'
            'AW_IOAPIC_IRQ_FIRED'
            'AW_IOAPIC_IRQ_MONOTONIC_OK'
            'AW_IOAPIC_MASKED_STOPPED'
            'AW_IOAPIC_UNMASKED_RESUMED'
            'AW_IOAPIC_DELIVERY_PROOF_OK'
            # A uniprocessor machine: the MADT describes one CPU and there is
            # nothing to start, which must be reported as such rather than as a
            # bring-up that silently did nothing.
            'AW_SMP_CPUS described=1'
            'AW_SMP_NO_APPLICATION_PROCESSORS'
            # The bootstrap processor's GS-reachable per-CPU block exists before
            # the first interrupt, and its per-CPU timer and device counters were
            # driven by the ISRs that actually ran on it - not left at zero while
            # only the shared global counter moved.
            'AW_PERCPU_BSP_OK cpu=0 apic_id=0'
            'AW_PERCPU_PROOF_OK cpus=1'
            # Monotonic TSC clock, calibrated against the PIT.
            'AW_CLOCK_MONOTONIC_OK'
            'AW_CLOCK_CALIBRATED_OK khz='
            'AW_CLOCK_PROOF_OK'
            # Wall-clock date/time read from the CMOS RTC.
            'AW_RTC_PROOF_OK'
            # The native screen reader speaks the installer's first screen: the
            # accessible tree is validated and every control voiced in focus
            # order, with a state-change event - nonvisual delivery evidence.
            'AW_SR_SPEAK "Install Accessible Windows, dialog"'
            'AW_SR_SPEAK "Welcome to Accessible Windows setup"'
            'AW_SR_SPEAK "Language, combo box, collapsed, English"'
            'AW_SR_SPEAK "Enable screen reader at boot, check box, checked"'
            'AW_SR_SPEAK "Speech rate, slider, 40%"'
            'AW_SR_SPEAK "Install, button, 1 of 2, installs to the selected disk"'
            'AW_SR_SPEAK "Recovery options, button, 2 of 2"'
            'AW_SR_EVENT "not checked"'
            # Keyboard-only navigation with spoken feedback: Tab across the four
            # focus stops in order (skipping a heading, static text and a disabled
            # control), wrap from the last back to the first, then Shift+Tab back.
            'AW_SR_TAB "Enable screen reader at boot, check box, checked, 1 of 4"'
            'AW_SR_TAB "Speech rate, slider, 40%, 2 of 4"'
            'AW_SR_TAB "Install, button, 3 of 4"'
            'AW_SR_TAB "Recovery, button, 4 of 4"'
            'AW_SR_TAB_WRAP "Enable screen reader at boot, check box, checked, 1 of 4"'
            'AW_SR_SHIFT_TAB "Recovery, button, 4 of 4"'
            'AW_SR_NAV_PROOF_OK'
            'AW_SR_PROOF_OK'
            # The same control rendered to braille for a refreshable display:
            # semantic node -> utterance -> Grade 1 six-dot cells, checked against
            # the known pattern (capital sign, install, comma, space, button).
            'AW_BRAILLE_CELLS 20 0a 1d 0e 1e 01 07 07 02 00 03 25 1e 1e 15 1d'
            'AW_BRAILLE_PROOF_OK'
            # Rest of bring-up still clean.
            'AW_MEMORY_MAP_VALIDATE_OK'
            'AW_BOOTSTRAP_PAGE_ALLOC_OK'
            'AW_KERNEL_RANGE_PROTECTED_OK'
            'AW_CPU_NX_OK'
            'AW_PAGING_BASELINE_OK'
            'AW_NATIVE_FRAMEBUFFER_WRITE_OK'
            # The framebuffer text console: it renders a known glyph into the
            # handed-off linear framebuffer and reads every pixel of it back in the
            # framebuffer's own colour order, matching the scaled 8x8 font bitmap -
            # the first post-firmware output that survives on real hardware.
            'AW_FBCON_READY width=1280 height=800'
            'AW_FBCON_GLYPH_READBACK_OK char=A'
            'AW_FBCON_PROOF_OK'
            # PS/2 keyboard: real IRQ1 delivery and set-1 decode, proved by the
            # 8042's own 0xD2 injection (the exact path a keypress takes), plus a
            # mask/resume negative test. The input half of an accessible boot.
            'AW_KBD_CONTROLLER_OK'
            'AW_KBD_KEY name=space'
            'AW_KBD_KEY name=enter'
            'AW_KBD_KEY name=up'
            'AW_KBD_KEY name=down'
            'AW_KBD_MASKED_STOPPED'
            'AW_KBD_UNMASKED_RESUMED'
            'AW_KBD_PROOF_OK'
            # Accessible boot menu: keyboard-driven navigation and selection, the
            # same handler the live menu uses, voiced through the screen-reader
            # engine and rendered to the framebuffer with a visible focus bar.
            'AW_MENU_FOCUS index=1 name="System information"'
            'AW_MENU_SPEAK "System information, menu item, 2 of 4"'
            # ACPI power controls read from the FADT and the DSDT's \_S5 package.
            'AW_ACPI_POWER_CONTROL pm1a='
            'AW_MENU_SELECT name="System information"'
            'AW_MENU_WRAP_OK'
            'AW_MENU_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
            'AW_FBCON_FAIL'
            'AW_FBCON_UNAVAILABLE'
            'AW_KBD_FAIL'
            'AW_KBD_UNAVAILABLE'
            'AW_MENU_FAIL'
            'AW_MEMORY_PROTECTION_FAIL'
            'AW_MEMORY_PROTECTION_SKIPPED'
            'AW_VMM_FAIL'
            'AW_VMM_RUNTIME_MAP_FAIL'
            'AW_HEAP_FAIL'
            'AW_RING3_FAIL'
            'AW_SWAPGS_FAIL'
            'AW_SCHED_FAIL'
            'AW_PREEMPT_FAIL'
            'AW_RING3_PREEMPT_FAIL'
            'AW_CLOCK_FAIL'
            'AW_RTC_FAIL'
            'AW_UEFI_SR_FAIL'
            'AW_UEFI_MENU_FAIL'
            'AW_UEFI_SND_FAIL'
            'AW_SR_FAIL'
            'AW_BRAILLE_FAIL'
            'AW_GDT_SEGMENTS_FAIL'
            'AW_PERCPU_BSP_FAIL'
            'AW_PERCPU_FAIL'
            'AW_APIC_TIMER_NOT_FIRED'
            'AW_APIC_TIMER_MASK_INEFFECTIVE'
            'AW_APIC_TIMER_DID_NOT_RESUME'
            'AW_IOAPIC_UNAVAILABLE'
            'AW_IOAPIC_IRQ_NOT_FIRED'
            'AW_IOAPIC_MASK_INEFFECTIVE'
            'AW_IOAPIC_DID_NOT_RESUME'
            'AW_SMP_UNAVAILABLE'
            'AW_SMP_AP_NOT_ONLINE'
            'AW_SMP_TABLES_SHARED'
            'AW_SECURITY_BASELINE_GAP'
        )
    }
    @{
        # Four processors. "Online" is not a counter the bootstrap processor
        # increments: each AP reports the APIC ID it read from its own local
        # APIC and the tables it actually loaded, and those must all differ.
        Name     = 'smp'
        Features = @()
        QemuArgs = @('-smp', '4')
        # Four vCPUs under TCG, plus each application processor accumulating real
        # Local APIC timer ticks and the PIT/TSC calibration, do not reach
        # AW_NATIVE_KERNEL_IDLE inside the default budget: measured to need well
        # over the 90 s the fast paths use, so it is given room under the 300 s cap
        # Invoke-KernelBoot enforces. Raising this weakens no assertion - every
        # required marker below must still appear.
        TimeoutSeconds = 240
        Required = @(
            'AW_SMP_CPUS described=4 bsp_apic_id=0'
            'AW_SMP_AP_ONLINE cpu=1 apic_id=1 requested=1 tr=0x0000000000000018'
            'AW_SMP_AP_ONLINE cpu=2 apic_id=2 requested=2 tr=0x0000000000000018'
            'AW_SMP_AP_ONLINE cpu=3 apic_id=3 requested=3 tr=0x0000000000000018'
            'AW_SMP_ONLINE online=3 started=3'
            'AW_SMP_PER_CPU_TABLES_OK cpus=3'
            'AW_SMP_ALL_ONLINE'
            # Each online CPU owns a distinct GS-reachable per-CPU block whose
            # index and APIC id match what SMP bring-up recorded: four blocks for
            # the bootstrap processor plus its three application processors.
            'AW_PERCPU_BSP_OK cpu=0 apic_id=0'
            'AW_PERCPU_PROOF_OK cpus=4'
            'AW_CLOCK_PROOF_OK'
            # Each application processor armed its own Local APIC timer and took
            # real interrupts on it, counted into its own per-CPU block.
            'AW_PERCPU_AP_TIMER_OK aps=3'
            # The rest of bring-up must survive having other CPUs running.
            'AW_MEMORY_PROTECTION_PROOF_OK'
            'AW_VMM_RUNTIME_MAP_PROOF_OK'
            'AW_HEAP_PROOF_OK'
            'AW_RING3_PROOF_OK'
            'AW_SYSCALL_ABI_PROOF_OK version=1'
            'AW_SCHED_PROOF_OK threads=3 switches=29'
            'AW_APIC_TIMER_DELIVERY_PROOF_OK'
            'AW_IOAPIC_DELIVERY_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_SMP_UNAVAILABLE'
            'AW_SMP_AP_NOT_ONLINE'
            'AW_SMP_TABLES_SHARED'
            'AW_SMP_NO_APPLICATION_PROCESSORS'
            'AW_PERCPU_BSP_FAIL'
            'AW_PERCPU_FAIL'
            'AW_VMM_RUNTIME_MAP_FAIL'
            'AW_HEAP_FAIL'
            'AW_RING3_FAIL'
            'AW_SCHED_FAIL'
            'AW_CLOCK_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # MSI has no pin and no I/O APIC in the path: the device writes the
        # interrupt straight into the local APIC's message window. QEMU's `edu`
        # device is the only thing here that can be asked to send one without
        # first implementing a real controller's command protocol, so this
        # configuration - and only this one - builds the driver for it.
        Name     = 'msi-smoke'
        Features = @('msi-proof-device')
        QemuArgs = @('-device', 'edu')
        Required = @(
            'AW_MSI_DEVICE_FOUND'
            'AW_MSI_PROGRAMMED vector=0x0000000000000052 address=0x00000000fee00000 data=0x0000000000000052'
            'AW_MSI_FIRED'
            'AW_MSI_MONOTONIC_OK'
            'AW_MSI_MASKED_STOPPED'
            'AW_MSI_UNMASKED_RESUMED'
            'AW_MSI_DELIVERY_PROOF_OK'
            # The I/O APIC path must keep working with the device present.
            'AW_IOAPIC_DELIVERY_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_MSI_UNAVAILABLE'
            'AW_MSI_NOT_FIRED'
            'AW_MSI_MASK_INEFFECTIVE'
            'AW_MSI_DID_NOT_RESUME'
            # Nothing may arrive on a vector this kernel did not install, which
            # is what an INTx fallback slipping through would look like.
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # A real device driver: bring up legacy virtio-blk, read sector 0 through
        # one virtqueue, and check the bytes against the magic the test disk was
        # built with. The proof is the sector content, not a status register.
        Name     = 'virtio-blk'
        Features = @()
        QemuArgs = @(
            '-drive', "file=$vblkDisk,if=none,id=vblk,format=raw",
            '-device', 'virtio-blk-pci,drive=vblk,disable-modern=on'
        )
        Required = @(
            'AW_VIRTIO_BLK_FOUND'
            'AW_VIRTIO_BLK_CAPACITY sectors='
            'AW_VIRTIO_BLK_READ_OK sector=0'
            'AW_VIRTIO_BLK_PROOF_OK'
            # The filesystem, read from that same device: parse the FAT16 BPB,
            # find HELLO.TXT in the root directory, follow its cluster chain, and
            # match the bytes it was written with.
            'AW_FS_FILE_FOUND size=25'
            'AW_FS_READ_OK'
            'AW_FS_PROOF_OK'
            # A userland ELF read off that same filesystem, its PT_LOAD segment
            # mapped and run at CPL3: it reports 0xC0DE through SYS_REPORT and exits.
            'AW_USER_LOADER_MAP_OK entry='
            'AW_USER_LOADER_PROOF_OK'
            # Then two userland spinners loaded from the same disk, preemptively
            # scheduled at CPL3 - both counters advance under timer switching.
            'AW_USER_INIT_PROOF_OK'
            # Channel IPC between those programs through handles only, and the five
            # refusals of the handle model (foreign, forged, rights, kernel pointer,
            # use after close).
            'AW_HANDLE_SECURITY_PROOF_OK'
            'AW_IPC_PROOF_OK messages=32'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_VIRTIO_BLK_FAIL'
            'AW_VIRTIO_BLK_UNAVAILABLE'
            'AW_FS_FAIL'
            'AW_USER_LOADER_FAIL'
            'AW_USER_LOADER_UNAVAILABLE'
            'AW_USER_INIT_FAIL'
            'AW_USER_INIT_UNAVAILABLE'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # The second real driver: legacy virtio-net against QEMU's user-mode
        # (SLIRP) network. The proof is an ARP exchange, not a status bit - the
        # guest reads its own MAC, confirms the receive ring stays quiet while it
        # sends nothing, broadcasts "who has 10.0.2.2", and matches the gateway's
        # reply. `disable-modern=on` selects the transitional (I/O BAR) device the
        # driver speaks; the fixed `mac=` makes the config-space read deterministic.
        Name     = 'net'
        Features = @()
        QemuArgs = @(
            '-netdev', 'user,id=n0'
            '-device', 'virtio-net-pci,netdev=n0,disable-modern=on,mac=52:54:00:12:34:56'
        )
        Required = @(
            'AW_VIRTIO_NET_FOUND'
            'AW_VIRTIO_NET_MAC mac=52:54:00:12:34:56'
            'AW_VIRTIO_NET_QUIET_OK'
            'AW_VIRTIO_NET_ARP_SENT'
            'AW_VIRTIO_NET_ARP_REPLY_OK spa=10.0.2.2 sha='
            'AW_VIRTIO_NET_PROOF_OK'
            'AW_VIRTIO_NET_ICMP_SENT'
            'AW_VIRTIO_NET_ICMP_REPLY_OK src=10.0.2.2 id=ab01 seq=1'
            'AW_VIRTIO_NET_ICMP_PROOF_OK'
            'AW_VIRTIO_NET_DHCP_DISCOVER_SENT'
            'AW_VIRTIO_NET_DHCP_OFFER_OK yiaddr=10.0.2.15'
            'AW_VIRTIO_NET_DHCP_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_VIRTIO_NET_UNAVAILABLE'
            'AW_VIRTIO_NET_FAIL'
            'AW_VIRTIO_NET_ICMP_FAIL'
            'AW_VIRTIO_NET_DHCP_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # NVMe: the interface modern PCs boot their SSDs through, and the first
        # memory-mapped (not port-mapped) controller. Bring up the admin queue
        # pair, enable the controller, issue IDENTIFY CONTROLLER, and read the
        # model number it wrote back by DMA - real content, not a status bit. The
        # backing image is only ever read by this identify proof.
        Name     = 'nvme'
        Features = @()
        QemuArgs = @(
            '-drive', "file=$vblkDisk,if=none,id=nvm,format=raw"
            '-device', 'nvme,drive=nvm,serial=AWNVME01'
        )
        Required = @(
            'AW_NVME_FOUND'
            'AW_NVME_ENABLED depth='
            'AW_NVME_IDENTIFY_OK model=QEMU NVMe Ctrl'
            'AW_NVME_PROOF_OK'
            # Block I/O: namespace geometry, an I/O queue pair, and LBA 0 read by DMA
            # and recognised as the test disk's FAT16 boot sector.
            'AW_NVME_NAMESPACE blocks='
            'AW_NVME_IO_QUEUES_OK depth='
            'AW_NVME_READ_PROOF_OK fs=FAT16'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_NVME_UNAVAILABLE'
            'AW_NVME_IO_FAIL'
            'AW_NVME_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # Intel HD Audio: the machine's real audio path and the foundation for
        # spoken screen-reader output. Bring the controller out of reset, stand up
        # the CORB/RIRB command/response rings, find the codec, and read the codec's
        # vendor/device id back over the ring - real data the codec produced, not a
        # status bit. QEMU's ich6 `intel-hda` controller with an `hda-output` codec;
        # the `none` audio backend discards sound but the controller and codec still
        # run, which is all bring-up needs.
        Name     = 'hda'
        Features = @()
        QemuArgs = @(
            '-audiodev', 'none,id=snd0'
            '-device', 'intel-hda'
            '-device', 'hda-output,audiodev=snd0'
        )
        # The UEFI stage streams ~12 s of pre-recorded speech through HDA before the
        # kernel even loads, so this configuration needs more than the default
        # budget to reach idle. Raising it weakens no assertion.
        TimeoutSeconds = 150
        Required = @(
            # First, the firmware stage speaks through HDA, before the kernel: the
            # UEFI screen reader reaches the real audio codec (arXiv:1712.03186) and
            # streams pre-recorded PCM speech of the boot screen by DMA, so a blind
            # user hears the words on the machine's actual speakers, not the (often
            # absent) PC-speaker buzzer. Sound is not captured headless; the proof is
            # the codec answering and the clips streaming (link position advanced).
            'AW_UEFI_HDA_CODEC_ID vendor_device='
            'AW_UEFI_HDA_READY dac='
            'AW_UEFI_AUDIO_SPEAK bytes='
            # Then the kernel brings the same controller up again for the installer.
            'AW_HDA_FOUND'
            'AW_HDA_RESET_OK'
            'AW_HDA_CODEC_PRESENT'
            'AW_HDA_CODEC_ID vendor_device='
            'AW_HDA_PROOF_OK'
            # Then real audio DMA: walk the codec to a DAC + output pin, configure
            # them, program output stream 0 with a BDL over a PCM tone buffer, start
            # it, and prove the link position advances - the controller is actually
            # streaming samples from memory, not just holding a status bit.
            'AW_HDA_OUTPUT dac='
            'AW_HDA_STREAM_RUN'
            'AW_HDA_DMA_ADVANCED position='
            'AW_HDA_PLAYBACK_PROOF_OK'
            # Then real spoken output: the codec is set to the 24 kHz mono speech
            # format and one pre-recorded menu clip is streamed by DMA, the link
            # position advancing just like the tone - the accessible menu's voice,
            # proven to reach the codec (audibility on hardware is separate).
            'AW_HDA_SPEECH_READY'
            'AW_HDA_SPEECH_DMA_ADVANCED position='
            'AW_HDA_SPEECH_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_UEFI_HDA_UNAVAILABLE'
            'AW_UEFI_HDA_FAIL'
            'AW_HDA_UNAVAILABLE'
            'AW_HDA_FAIL'
            'AW_HDA_SPEECH_FAIL'
            'AW_HDA_SPEECH_UNAVAILABLE'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # AC'97 audio: a second self-built firmware-stage backend, so the spoken screen
        # reader works on machines whose codec is AC'97 rather than Intel HDA (and what
        # several virtual machines expose). No HDA device is present here, so the audio
        # layer falls through HDA to AC'97: it finds the controller on PCI, brings up its
        # NAM/NABM I/O windows, and streams the boot-screen speech clips by bus-master DMA
        # over a Buffer Descriptor List. The `none` audiodev discards the sound, but the bus
        # master still runs the descriptor list, which is all the DMA path needs to prove.
        Name           = 'ac97'
        Features       = @()
        QemuArgs       = @(
            '-audiodev', 'none,id=snd0'
            '-device', 'AC97,audiodev=snd0'
        )
        # The UEFI stage streams several seconds of pre-recorded speech through AC'97 before
        # the kernel loads, so this configuration needs more than the default budget.
        TimeoutSeconds = 150
        Required       = @(
            'AW_UEFI_AC97_READY nam='
            'AW_UEFI_AUDIO_BACKEND channel=ac97'
            'AW_UEFI_AUDIO_SPEAK bytes='
            'AW_UEFI_SR_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden      = @(
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # A real 16550 UART console on COM1: an internal loopback test proves the
        # device, then a banner is emitted on the real line and checked in the
        # host-side serial log - output that actually left the guest.
        Name           = 'serial'
        Features       = @()
        Serial         = "file:$serialFile"
        Required       = @(
            'AW_SERIAL_LOOPBACK_OK byte=0xae'
            'AW_SERIAL_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden      = @(
            'AW_SERIAL_UNAVAILABLE'
            'AW_SERIAL_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
        SerialFile     = $serialFile
        SerialContains = 'AW-SERIAL-CONSOLE-OK'
    }
    @{
        Name     = 'exception-smoke'
        Features = @('exception-smoke-test')
        Required = @(
            'AW_EXCEPTION_SMOKE_TRIGGER vector=6'
            'AW_NATIVE_EXCEPTION vector=6 name=invalid-opcode'
            'AW_INVALID_OPCODE_HANDLER_OK'
        )
        Forbidden = @('AW_NATIVE_KERNEL_PANIC')
    }
    @{
        Name     = 'double-fault-smoke'
        Features = @('exception-smoke-test', 'double-fault-smoke-test')
        Required = @(
            'AW_DOUBLE_FAULT_SMOKE_ARMED'
            'AW_NATIVE_EXCEPTION vector=8 name=double-fault'
            'AW_DOUBLE_FAULT_IST_OK'
        )
        Forbidden = @('AW_DOUBLE_FAULT_IST_FAIL', 'AW_NATIVE_KERNEL_PANIC')
    }
    @{
        # One application processor deliberately double-faults, to prove the #DF
        # resolves on that CPU's own per-CPU IST1 rather than the bootstrap
        # processor's. The handler range-checks the frame against the current CPU's
        # IST bounds (read from its per-CPU block), so AW_DOUBLE_FAULT_IST_OK here
        # means the AP faulted onto its own stack. The AP faults before reporting
        # online, so bring-up records it offline and the per-CPU timer proof skips
        # it - hence AW_SMP_AP_NOT_ONLINE and AW_NATIVE_EXCEPTION are expected and
        # not forbidden. The bootstrap processor still reaches idle.
        Name     = 'ap-double-fault-smoke'
        Features = @('ap-double-fault-smoke-test')
        QemuArgs = @('-smp', '2')
        TimeoutSeconds = 180
        Required = @(
            'AW_AP_DOUBLE_FAULT_SMOKE cpu=1'
            'AW_NATIVE_EXCEPTION vector=8 name=double-fault'
            'AW_DOUBLE_FAULT_IST_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @('AW_DOUBLE_FAULT_IST_FAIL', 'AW_NATIVE_KERNEL_PANIC')
    }
    @{
        # Cooperative scheduling on an application processor: one AP runs two
        # kernel threads that context switch and take turns, before it goes on to
        # report online and idle under its own timer like any other AP. This proves
        # threads run on a CPU other than the bootstrap processor - the AP no longer
        # only parks in hlt.
        Name     = 'ap-scheduler-smoke'
        Features = @('ap-scheduler-smoke-test')
        QemuArgs = @('-smp', '2')
        TimeoutSeconds = 180
        Required = @(
            'AW_AP_SCHED_BEGIN cpu=1'
            'AW_AP_SCHED_PROOF_OK cpu=1 threads=2'
            'AW_SMP_ALL_ONLINE'
            'AW_PERCPU_PROOF_OK cpus=2'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_AP_SCHED_FAIL'
            'AW_SCHED_FAIL'
            'AW_PERCPU_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # A real SATA controller: bring up an AHCI HBA, find the port with a disk,
        # and read LBA 0 by DMA (READ DMA EXT), checking the 0x55AA boot signature
        # the disk actually returned. This is the controller model VMware and most
        # physical PCs expose SATA disks through. An explicit ich9-ahci carries the
        # disk (q35's built-in AHCI has no disk); the driver scans every HBA.
        Name     = 'ahci'
        Features = @()
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=ahcidisk,file=$vblkDisk,format=raw"
            '-device', 'ide-hd,drive=ahcidisk,bus=sata0.0'
        )
        Required = @(
            'AW_AHCI_FOUND'
            'AW_AHCI_PORT_PRESENT'
            'AW_AHCI_READ_OK sector=0'
            'AW_AHCI_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_AHCI_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # AHCI write: write a known pattern to LBA 0 of a dedicated *scratch* disk by
        # DMA (WRITE DMA EXT), then read it back and confirm the bytes round-tripped.
        # The scratch disk is blank and never a data disk, since the proof overwrites
        # LBA 0. Gated behind ahci-write-smoke-test so the normal path never writes.
        Name     = 'ahci-write'
        Features = @('ahci-write-smoke-test')
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=scratch,file=$ahciScratch,format=raw"
            '-device', 'ide-hd,drive=scratch,bus=sata0.0'
        )
        Required = @(
            'AW_AHCI_PORT_PRESENT'
            'AW_AHCI_WRITE_ISSUED sector=0'
            'AW_AHCI_WRITE_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_AHCI_WRITE_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # GPT parser: read the partition table off a real GPT disk on AHCI, validate
        # the header signature and CRC32, and find the first partition (the ESP).
        # Read-only. The disk is a full GPT image (protective MBR + GPT + FAT16 ESP).
        Name     = 'gpt'
        Features = @()
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=gptdisk,file=$gptDisk,format=raw"
            '-device', 'ide-hd,drive=gptdisk,bus=sata0.0'
        )
        Required = @(
            'AW_GPT_HEADER_OK'
            'AW_GPT_PARTITION index=0'
            'AW_GPT_PROOF_OK'
            # The full storage stack: read HELLO.TXT from the ESP's own FAT16, at the
            # partition offset the GPT reported (AHCI -> GPT -> partition -> FAT).
            'AW_FSPART_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_GPT_FAIL'
            'AW_FSPART_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # Filesystem write: create a file on a FAT16 volume (allocate a free cluster,
        # write the data, chain the FAT in every copy, add a root-directory entry) and
        # read it back through the ordinary reader. This is the installer foundation.
        # Its own scratch FAT16 disk, recreated each run: it modifies the filesystem,
        # so it must never touch a data disk. Gated behind fat-write-smoke-test.
        Name     = 'fat-write'
        Features = @('fat-write-smoke-test')
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=fatscratch,file=$fatScratch,format=raw"
            '-device', 'ide-hd,drive=fatscratch,bus=sata0.0'
        )
        Required = @(
            'AW_FATWRITE_BEGIN'
            'AW_FATWRITE_WROTE'
            'AW_FATWRITE_READBACK_OK'
            'AW_FATWRITE_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_FATWRITE_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # GPT write: partition a blank scratch disk - write a protective MBR, a
        # primary and backup GPT header (each CRC32-checked) and an entry array with
        # one ESP - then read the table back and validate both headers' CRCs and the
        # ESP entry. This is the installer's partitioning half. After the write, the
        # ordinary AHCI read (AW_AHCI_PROOF_OK) and GPT read (AW_GPT_PROOF_OK) run
        # against the table we just wrote, so this also proves the reader on it.
        # Its own blank scratch disk, recreated each run: it overwrites the whole
        # partition table, so it must never touch a data disk. Gated behind
        # gpt-write-smoke-test.
        Name     = 'gpt-write'
        Features = @('gpt-write-smoke-test')
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=gptscratch,file=$gptScratch,format=raw"
            '-device', 'ide-hd,drive=gptscratch,bus=sata0.0'
        )
        Required = @(
            'AW_GPTWRITE_BEGIN sectors=16384'
            'AW_GPTWRITE_WROTE'
            'AW_GPTWRITE_HEADER_OK'
            'AW_GPTWRITE_BACKUP_OK'
            'AW_GPTWRITE_PROOF_OK'
            # The reader validates the table we wrote: signature + CRC + ESP entry.
            'AW_GPT_HEADER_OK'
            'AW_GPT_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_GPTWRITE_FAIL'
            'AW_GPT_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # FAT16 format (mkfs): write a fresh, empty FAT16 filesystem onto a blank
        # scratch disk - boot sector/BPB, two FATs with their reserved entries, a
        # zeroed root directory - then create a file in it with the ordinary writer
        # and read it back with the ordinary reader. Together with the GPT writer,
        # the kernel can now build a whole disk from blank: the installer's format
        # step. Its own blank scratch disk, recreated each run; gated behind
        # fat-format-smoke-test so it never touches a data disk.
        Name     = 'mkfs'
        Features = @('fat-format-smoke-test')
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=mkfsscratch,file=$mkfsScratch,format=raw"
            '-device', 'ide-hd,drive=mkfsscratch,bus=sata0.0'
        )
        Required = @(
            'AW_MKFS_BEGIN sectors=16384'
            'AW_MKFS_FORMATTED'
            'AW_MKFS_READBACK_OK'
            'AW_MKFS_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_MKFS_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        # Disk-build capstone: on one blank scratch disk the kernel writes a GPT,
        # formats the ESP as FAT16, writes a file, then reads it back through the
        # whole stack it just built (GPT -> partition -> FAT -> file) - what an
        # installer does to provision a target disk. Its own blank scratch disk,
        # recreated each run; it partitions and formats the whole disk, so it must
        # never touch a data disk. Gated behind disk-build-smoke-test.
        Name     = 'disk-build'
        Features = @('disk-build-smoke-test')
        QemuArgs = @(
            '-device', 'ich9-ahci,id=sata0'
            '-drive', "if=none,id=buildscratch,file=$buildScratch,format=raw"
            '-device', 'ide-hd,drive=buildscratch,bus=sata0.0'
        )
        Required = @(
            'AW_DISKBUILD_BEGIN sectors=16384'
            'AW_DISKBUILD_PARTITIONED'
            'AW_DISKBUILD_FORMATTED'
            'AW_DISKBUILD_WROTE'
            'AW_DISKBUILD_READBACK_OK'
            'AW_DISKBUILD_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_DISKBUILD_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
)

$failures = @()

# ACPI power control (roadmap Phase 3). Power off: the kernel reads the FADT and
# \_S5, enters S5, and QEMU must then exit by itself - only a real soft-off
# transition ends the VM before the timeout. Reset: the kernel resets through the
# FADT reset register; the VM reboots (no -no-reboot) and the second boot, finding
# the CMOS flag the first one left, reports - two idle markers, one proof.
$configurations += @(
    @{
        # NVMe write: a pattern to LBA 2 of a blank scratch disk, read back by DMA
        # into a different buffer. Never a data disk.
        Name     = 'nvme-write'
        Features = @('nvme-write-smoke-test')
        QemuArgs = @(
            '-drive', "file=$nvmeScratch,if=none,id=nvm,format=raw"
            '-device', 'nvme,drive=nvm,serial=AWNVME02'
        )
        Required = @('AW_NVME_IO_QUEUES_OK depth=', 'AW_NVME_WRITE_PROOF_OK lba=2', 'AW_NATIVE_KERNEL_IDLE')
        Forbidden = @('AW_NVME_IO_FAIL', 'AW_NVME_FAIL', 'AW_NATIVE_EXCEPTION')
    }
    @{
        # Destructive xHCI controller ownership proof. QEMU-only: halt/reset the
        # emulated controller, install bounded DMA structures, Run, Stop, and
        # require every transition. No physical image enables this feature.
        Name     = 'xhci-smoke'
        Features = @('xhci-smoke-test')
        QemuArgs = @(
            '-device', 'qemu-xhci'
        )
        Required = @(
            'AW_XHCI_PCI_DISCOVERY_PROOF_OK'
            'AW_XHCI_MMIO_CAP_PROOF_OK'
            'AW_XHCI_SMOKE_BEGIN'
            'AW_XHCI_SMOKE_HALTED'
            'AW_XHCI_SMOKE_RESET_OK'
            'AW_XHCI_SMOKE_DMA_READY'
            'AW_XHCI_SMOKE_RUNNING'
            'AW_XHCI_SMOKE_STOPPED'
            'AW_XHCI_SMOKE_PROOF_OK'
            'AW_NATIVE_KERNEL_IDLE'
        )
        Forbidden = @(
            'AW_XHCI_SMOKE_FAIL'
            'AW_NATIVE_EXCEPTION'
            'AW_NATIVE_KERNEL_PANIC'
        )
    }
    @{
        Name           = 'acpi-poweroff'
        Features       = @('acpi-poweroff-test')
        ExpectSelfExit = $true
        Required       = @(
            'AW_ACPI_POWER_CONTROL pm1a='
            'AW_NATIVE_KERNEL_IDLE'
            'AW_ACPI_POWEROFF_ISSUED'
        )
        Forbidden      = @('AW_ACPI_POWEROFF_FAIL', 'AW_ACPI_POWER_CONTROL_UNAVAILABLE', 'AW_NATIVE_EXCEPTION')
    }
    @{
        Name           = 'acpi-reset'
        Features       = @('acpi-reset-test')
        AllowReboot    = $true
        TimeoutSeconds = 150
        Required       = @(
            'AW_ACPI_POWER_CONTROL pm1a='
            'AW_ACPI_RESET_ISSUED'
            'AW_POWER_RESET_PATH fadt_io'
            'AW_ACPI_RESET_PROOF_OK second_boot=1'
        )
        RequiredCount  = @{ 'AW_NATIVE_KERNEL_IDLE' = 2 }
        Forbidden      = @('AW_POWER_RESET_PATH i8042', 'AW_ACPI_POWER_CONTROL_UNAVAILABLE', 'AW_NATIVE_EXCEPTION')
    }
)
if ($Only.Count -gt 0) {
    $configurations = @($configurations | Where-Object { $Only -contains $_.Name })
    if ($configurations.Count -eq 0) { throw "no configuration matched -Only: $($Only -join ', ')" }
}

foreach ($configuration in $configurations) {
    Write-Host "== $($configuration.Name) =="
    # Assigned explicitly: `$x = if (...) { ... } else { @() }` yields $null,
    # which binds to [string[]] as a single empty argument and makes QEMU treat
    # it as an extra disk image.
    [string[]]$qemuArgs = @()
    if ($configuration.ContainsKey('QemuArgs')) { $qemuArgs = $configuration.QemuArgs }
    $serial = 'none'
    if ($configuration.ContainsKey('Serial')) { $serial = $configuration.Serial }
    # The guest halts forever once it reaches AW_NATIVE_KERNEL_IDLE, so every run
    # burns its whole timeout before QEMU is killed and the log is read: the
    # timeout is therefore a *budget for the markers to appear*, not a deadline
    # the guest races to beat. Most configs reach idle well inside the default,
    # but a config may set its own larger budget when it legitimately needs one
    # (SMP brings up four vCPUs and lets each application processor accumulate
    # real timer ticks under TCG, which does not fit the default).
    $cfgTimeout = $TimeoutSeconds
    if ($configuration.ContainsKey('TimeoutSeconds')) {
        $cfgTimeout = [math]::Max($TimeoutSeconds, [int]$configuration.TimeoutSeconds)
    }
    $allowReboot = $configuration.ContainsKey('AllowReboot') -and $configuration.AllowReboot
    $result = & $boot -Name $configuration.Name -Features $configuration.Features `
        -QemuArgs $qemuArgs -Serial $serial -Qemu $Qemu -TimeoutSeconds $cfgTimeout -AllowReboot:$allowReboot

    $missing = @($configuration.Required | Where-Object { -not $result.Text.Contains($_) })
    $present = @($configuration.Forbidden | Where-Object { $result.Text.Contains($_) })
    # Power off is proved by the VM ending itself, not by a marker alone.
    if ($configuration.ContainsKey('ExpectSelfExit') -and $configuration.ExpectSelfExit -and -not $result.ExitedOnItsOwn) {
        $missing += 'qemu-exited-on-its-own'
    }
    # Some proofs need a marker several times (a reset shows two boots).
    if ($configuration.ContainsKey('RequiredCount')) {
        foreach ($entry in $configuration.RequiredCount.GetEnumerator()) {
            $count = ([regex]::Matches($result.Text, [regex]::Escape($entry.Key))).Count
            if ($count -lt $entry.Value) { $missing += "$($entry.Key) x$($entry.Value) (saw $count)" }
        }
    }

    # A config may also assert on the host-side serial log: output the guest
    # actually pushed out of COM1, not just a debug-console marker.
    if ($configuration.ContainsKey('SerialFile')) {
        $serialText = if (Test-Path -LiteralPath $configuration.SerialFile) {
            Get-Content -LiteralPath $configuration.SerialFile -Raw
        } else { '' }
        if (-not $serialText.Contains($configuration.SerialContains)) {
            $missing += "serial:$($configuration.SerialContains)"
        }
    }

    foreach ($marker in $missing) {
        $failures += "$($configuration.Name): missing '$marker'"
    }
    foreach ($marker in $present) {
        $failures += "$($configuration.Name): forbidden '$marker' present"
    }

    if ($missing.Count -eq 0 -and $present.Count -eq 0) {
        Write-Host "   PASS  ($($configuration.Required.Count) markers)  log: $($result.Log)"
    } else {
        Write-Host "   FAIL  log: $($result.Log)"
    }
}

if ($failures.Count -gt 0) {
    throw "Boot proof failures:`n  " + ($failures -join "`n  ")
}

Write-Host ''
Write-Host 'ALL BOOT PROOFS PASS (QEMU/OVMF q35, tcg, -cpu max).'
Write-Host 'Physical hardware and GPT disk-image boot remain separate validations.'
