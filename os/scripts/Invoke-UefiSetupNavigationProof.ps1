#Requires -Version 7.0
<#
.SYNOPSIS
    Prove complete keyboard navigation and spoken focus in Accessible Windows UEFI Setup.

.DESCRIPTION
    QEMU monitor commands are synchronized to fresh debug markers. The proof visits
    every Setup subtree, a real Boot#### entry, Secure Boot status, both power
    confirmation pages, then chooses Boot normally and requires the kernel to idle.
#>
[CmdletBinding()]
param(
    [string]$Qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$runner = Join-Path $PSScriptRoot 'Invoke-KernelBoot.ps1'

$steps = @(
    'AW_UEFI_SR_READY|||sendkey ret'
    'AW_UEFI_SETUP_READY|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=Main index=0 label="System information"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=SystemInformation index=0 label="Firmware vendor"|||sendkey esc'
    'AW_UEFI_SETUP_FOCUS page=Main index=0 label="System information"|||sendkey esc'

    'AW_UEFI_SETUP_FOCUS page=Root index=0 label="Main"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=1 label="Advanced"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=Advanced index=0 label="CPU configuration"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=CpuConfiguration index=0 label="Architecture x86 64"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=CpuConfiguration index=1 label="Virtualization capability"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=CpuConfiguration index=2 label="Back"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=Advanced index=0 label="CPU configuration"|||sendkey esc'

    'AW_UEFI_SETUP_FOCUS page=Root index=0 label="Main"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=1 label="Advanced"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=2 label="Boot"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=Boot index=0 label="Boot option priorities"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=BootPriorities index=0|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=BootEntry index=0 label="Boot now"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=BootEntry index=1 label="Set as default"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=BootEntry index=2 label="Back"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=BootPriorities index=0|||sendkey esc'
    'AW_UEFI_SETUP_FOCUS page=Boot index=0 label="Boot option priorities"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Boot index=1 label="Boot normally"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Boot index=2 label="Back"|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=Root index=0 label="Main"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=1 label="Advanced"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=2 label="Boot"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=3 label="Security"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=Security index=0 label="Secure Boot"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=SecureBoot index=0 label="Secure Boot status"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SecureBoot index=1 label="Setup mode"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SecureBoot index=2 label="Back"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=Security index=0 label="Secure Boot"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Security index=1 label="Back"|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=Root index=0 label="Main"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=1 label="Advanced"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=2 label="Boot"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=3 label="Security"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=4 label="Save and Exit"|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=SaveExit index=0 label="Boot normally"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=1 label="Restart"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=ConfirmRestart index=0 label="Confirm restart"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=ConfirmRestart index=1 label="Cancel"|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=SaveExit index=0 label="Boot normally"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=1 label="Restart"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=2 label="Shut down"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=ConfirmShutdown index=0 label="Confirm shut down"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=ConfirmShutdown index=1 label="Cancel"|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=SaveExit index=0 label="Boot normally"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=1 label="Restart"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=2 label="Shut down"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=3 label="Back"|||sendkey ret'

    'AW_UEFI_SETUP_FOCUS page=Root index=0 label="Main"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=1 label="Advanced"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=2 label="Boot"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=3 label="Security"|||sendkey down'
    'AW_UEFI_SETUP_FOCUS page=Root index=4 label="Save and Exit"|||sendkey ret'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=0 label="Boot normally"|||sendkey ret'
)

$params = @{
    Name = 'setup-navigation'
    Qemu = $Qemu
    TimeoutSeconds = 300
    QemuArgs = @(
        '-audiodev', 'none,id=snd0',
        '-device', 'intel-hda',
        '-device', 'hda-output,audiodev=snd0'
    )
    MonitorScript = $steps
}
& $runner @params
if ($LASTEXITCODE -ne 0) {
    throw "Invoke-KernelBoot setup-navigation failed with exit code $LASTEXITCODE"
}

$log = Join-Path (Split-Path $PSScriptRoot -Parent) 'target/boot-evidence/setup-navigation/debug.log'
if (-not (Test-Path -LiteralPath $log -PathType Leaf)) {
    throw "Missing setup navigation evidence: $log"
}
$text = ((Get-Content -LiteralPath $log -Raw) -replace "`0", '')

$required = @(
    'AW_UEFI_SETUP_SPEECH_MODE mode=real'
    'AW_UEFI_SETUP_BOOT_OPTIONS count='
    'AW_UEFI_SETUP_FOCUS page=SystemInformation index=0 label="Firmware vendor"'
    'AW_UEFI_SETUP_FOCUS page=CpuConfiguration index=1 label="Virtualization capability"'
    'AW_UEFI_SETUP_FOCUS page=BootPriorities index=0'
    'AW_UEFI_SETUP_FOCUS page=BootEntry index=1 label="Set as default"'
    'AW_UEFI_SETUP_FOCUS page=SecureBoot index=1 label="Setup mode"'
    'AW_UEFI_SETUP_FOCUS page=ConfirmRestart index=1 label="Cancel"'
    'AW_UEFI_SETUP_FOCUS page=ConfirmShutdown index=1 label="Cancel"'
    'AW_UEFI_SETUP_FOCUS page=SaveExit index=3 label="Back"'
    'AW_UEFI_HDA_SPELL char='
    'AW_UEFI_SETUP_ACTION action=boot_normally reason=user'
    'AW_UEFI_SETUP_PROOF_OK'
    'AW_NATIVE_KERNEL_IDLE'
)
$forbidden = @(
    'AW_UEFI_SETUP_SPEECH_MODE mode=fallback'
    'AW_UEFI_SETUP_FAIL'
    'AW_UEFI_HDA_FAIL'
    'AW_NATIVE_EXCEPTION'
    'AW_NATIVE_KERNEL_PANIC'
)

$missing = @($required | Where-Object { -not $text.Contains($_) })
if ($missing.Count -gt 0) {
    throw "Setup navigation proof missing: $($missing -join ', ')"
}
$presentForbidden = @($forbidden | Where-Object { $text.Contains($_) })
if ($presentForbidden.Count -gt 0) {
    throw "Setup navigation proof contains forbidden markers: $($presentForbidden -join ', ')"
}

$bootCountMatch = [regex]::Match($text, 'AW_UEFI_SETUP_BOOT_OPTIONS count=(\d+)')
if (-not $bootCountMatch.Success -or [int]$bootCountMatch.Groups[1].Value -lt 1) {
    throw 'Setup navigation proof did not expose at least one real Boot#### option'
}

$focusCount = ([regex]::Matches($text, 'AW_UEFI_SETUP_FOCUS')).Count
$spellCount = ([regex]::Matches($text, 'AW_UEFI_HDA_SPELL char=')).Count
Write-Host "UEFI_SETUP_NAVIGATION_FOCUS_EVENTS=$focusCount"
Write-Host "UEFI_SETUP_NAVIGATION_SPELL_EVENTS=$spellCount"
Write-Host 'UEFI_SETUP_NAVIGATION_PROOF=PASS'
