#Requires -Version 7.0
<#
.SYNOPSIS
    Build a real bootable UEFI disk image (GPT + FAT16 ESP) from the loader and
    kernel, and optionally prove it boots under QEMU/OVMF.

.DESCRIPTION
    The proof harness boots from QEMU's virtual FAT, which never yields a
    standalone file. This stages the normal-configuration loader and kernel into
    an EFI System Partition and wraps it in a GPT disk image with
    scripts/build_bootable_image.py (pure Python, no external imaging tools).

    The result, dist/accessible-windows-uefi-x86_64.img, boots in QEMU/OVMF as a
    normal disk and can be written to a USB stick (e.g. with Rufus in "DD"/image
    mode, or `dd`). It is a boot prototype, not an installer.

    Evidence rule (dossier section 1.1): a build is never a PASS. With -Verify
    the image is booted and AW_NATIVE_KERNEL_IDLE is required with no
    AW_NATIVE_EXCEPTION / AW_NATIVE_KERNEL_PANIC.
#>
[CmdletBinding()]
param(
    [string]$Out = 'dist/accessible-windows-uefi-x86_64.img',
    [string]$Qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe',
    [switch]$Verify,
    [ValidateRange(5, 300)][int]$TimeoutSeconds = 40
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command $($Arguments -join ' ') failed ($LASTEXITCODE)" }
}

$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    Invoke-Checked cargo @('build', '--locked', '--manifest-path', 'boot/uefi/Cargo.toml',
        '--target', 'x86_64-unknown-uefi', '--release')
    Invoke-Checked cargo @('build', '--locked', '--manifest-path', 'kernel/x86_64/Cargo.toml',
        '--target', 'x86_64-unknown-none', '--release')

    $sysroot = & rustc --print sysroot
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve Rust sysroot' }
    $objcopy = @(Get-ChildItem -LiteralPath (Join-Path $sysroot 'lib/rustlib') -Recurse -Filter llvm-objcopy.exe)
    if ($objcopy.Count -lt 1) { throw 'Install llvm-tools-preview: rustup component add llvm-tools-preview' }

    # Stage the ESP under target/ (a short path, kept out of git).
    $esp = Join-Path $repo 'target/bootimage-esp'
    if (Test-Path -LiteralPath $esp) { Remove-Item -LiteralPath $esp -Recurse -Force }
    New-Item -ItemType Directory -Path (Join-Path $esp 'EFI/BOOT') -Force | Out-Null

    Copy-Item -LiteralPath 'boot/uefi/target/x86_64-unknown-uefi/release/aw-uefi-boot.efi' `
        -Destination (Join-Path $esp 'EFI/BOOT/BOOTX64.EFI') -Force
    Invoke-Checked $objcopy[0].FullName @('-O', 'binary',
        'kernel/x86_64/target/x86_64-unknown-none/release/aw-kernel-x86_64',
        (Join-Path $esp 'KERNEL.BIN'))

    $outDir = Split-Path $Out -Parent
    if ($outDir -and -not (Test-Path -LiteralPath $outDir)) {
        New-Item -ItemType Directory -Path $outDir -Force | Out-Null
    }

    $python = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction Stop)
    Invoke-Checked $python.Source @('scripts/build_bootable_image.py', $Out, $esp)
    Write-Host "Built $Out"

    if (-not $Verify) { return }

    # Boot the image and require the idle marker with no fault.
    $share = Join-Path (Split-Path $Qemu -Parent) 'share'
    $code = Join-Path $share 'edk2-x86_64-code.fd'
    $vars = Join-Path $repo 'target/bootimage-vars.fd'
    Copy-Item -LiteralPath (Join-Path $share 'edk2-i386-vars.fd') -Destination $vars -Force
    $log = Join-Path $repo 'target/bootimage-boot.log'
    if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log -Force }

    $start = [System.Diagnostics.ProcessStartInfo]::new($Qemu)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    foreach ($a in @(
            '-machine', 'q35', '-accel', 'tcg', '-cpu', 'max', '-m', '256M',
            '-display', 'none', '-serial', 'none', '-monitor', 'none',
            '-no-reboot', '-net', 'none', '-debugcon', "file:$log",
            '-drive', "if=pflash,format=raw,readonly=on,file=$code",
            '-drive', "if=pflash,format=raw,file=$vars",
            '-drive', "format=raw,file=$(Join-Path $repo $Out)")) {
        $start.ArgumentList.Add($a)
    }
    $process = [System.Diagnostics.Process]::Start($start)
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) { $process.Kill(); $process.WaitForExit() }

    if (-not (Test-Path -LiteralPath $log)) { throw 'Image produced no debug log.' }
    $text = (Get-Content -LiteralPath $log -Raw) -replace "`0", ''
    $forbidden = @(@('AW_NATIVE_EXCEPTION', 'AW_NATIVE_KERNEL_PANIC') | Where-Object { $text.Contains($_) })
    if (-not $text.Contains('AW_NATIVE_KERNEL_IDLE')) {
        throw "Image did not reach AW_NATIVE_KERNEL_IDLE. Log: $log"
    }
    if ($forbidden.Count -gt 0) { throw "Image emitted forbidden markers: $($forbidden -join ', ')" }
    Write-Host "VERIFIED: image boots to AW_NATIVE_KERNEL_IDLE with no fault. Log: $log"
}
finally {
    Pop-Location
}
