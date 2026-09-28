#Requires -Version 7.0
<#
.SYNOPSIS
    Full local verification on Windows: workspace quality gates, then every
    bare-metal boot proof under QEMU/OVMF.

.DESCRIPTION
    Gates and proofs are kept separate on purpose. The gates below can all pass
    on a kernel that does not boot; only `Invoke-BootProofs.ps1` asserts what
    the CPU actually did, and it owns the marker lists so there is a single
    place to update when a proof changes.

    This does not validate the raw GPT image, installation, or physical
    hardware - those are separate validations.
#>
[CmdletBinding()]
param(
    [string]$Qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe',
    [ValidateRange(5, 300)][int]$BootTimeoutSeconds = 90,
    # Skip the host-side quality gates and run only the boot proofs.
    [switch]$ProofsOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    if (-not $ProofsOnly) {
        Invoke-Checked cargo @('fmt', '--all', '--', '--check')
        Invoke-Checked cargo @('check', '--locked', '--workspace', '--all-targets')
        Invoke-Checked cargo @('test', '--locked', '--workspace')
        Invoke-Checked cargo @('clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings')

        # The kernel crate is excluded from the workspace, so `clippy
        # --workspace` never sees it. Lint every feature combination that ships.
        Push-Location 'kernel/x86_64'
        try {
            foreach ($features in @('', 'msi-proof-device', 'exception-smoke-test', 'exception-smoke-test,double-fault-smoke-test', 'ap-double-fault-smoke-test', 'ap-scheduler-smoke-test', 'ahci-write-smoke-test', 'fat-write-smoke-test', 'gpt-write-smoke-test', 'fat-format-smoke-test', 'disk-build-smoke-test', 'nvme-write-smoke-test', 'xhci-smoke-test', 'acpi-poweroff-test', 'acpi-reset-test')) {
                $arguments = @('clippy', '--locked', '--target', 'x86_64-unknown-none')
                if ($features) { $arguments += @('--features', $features) }
                $arguments += @('--', '-D', 'warnings')
                Invoke-Checked cargo $arguments
            }
        } finally { Pop-Location }
    }

    & (Join-Path $PSScriptRoot 'Invoke-BootProofs.ps1') -Qemu $Qemu -TimeoutSeconds $BootTimeoutSeconds
} finally { Pop-Location }
