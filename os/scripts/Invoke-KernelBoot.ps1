#Requires -Version 7.0
<#
.SYNOPSIS
    Builds the UEFI loader + native kernel and boots them under QEMU/OVMF,
    returning the captured debug-console log.

.DESCRIPTION
    Single reusable harness for every bare-metal proof in this repository.
    One invocation = one kernel feature configuration = one QEMU run.

    Evidence rule (dossier section 1.1): a build success is never a PASS.
    The caller must assert on markers found in the returned log.
#>
[CmdletBinding()]
param(
    # Kernel cargo feature set for this run. Empty = normal boot path.
    [string[]]$Features = @(),

    # Label used for the per-run evidence directory.
    [string]$Name = 'normal',

    [string]$Qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe',

    [ValidateRange(5, 300)][int]$TimeoutSeconds = 40,

    # Skip cargo build and reuse the artifacts already staged for this run name.
    [switch]$NoBuild,

    # Extra QEMU arguments (e.g. -smp 2).
    [string[]]$QemuArgs = @(),

    # COM1 backend for QEMU's `-serial` (default 'none'). Set to e.g.
    # "file:C:\path\com1.log" to route the serial console to a file.
    [string]$Serial = 'none',

    # Optional deterministic QEMU HMP keyboard script. Each entry is
    # <debug marker>|||<monitor command>. A command is sent only after a NEW
    # occurrence of its marker appears in debug.log.
    [string[]]$MonitorScript = @(),

    # Let a guest reset reboot the VM instead of ending QEMU (reset proofs).
    [switch]$AllowReboot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

$repo = Split-Path $PSScriptRoot -Parent
$run = Join-Path $repo "target/boot-evidence/$Name"
$esp = Join-Path $run 'esp'

Push-Location $repo
try {
    if (-not $NoBuild) {
        if (Test-Path -LiteralPath $run) { Remove-Item -LiteralPath $run -Recurse -Force }
        New-Item -ItemType Directory -Path (Join-Path $esp 'EFI/BOOT') -Force | Out-Null

        Invoke-Checked cargo @('build', '--locked', '--manifest-path', 'boot/uefi/Cargo.toml',
            '--target', 'x86_64-unknown-uefi', '--release')

        $kernelArgs = @('build', '--locked', '--manifest-path', 'kernel/x86_64/Cargo.toml',
            '--target', 'x86_64-unknown-none', '--release')
        if ($Features.Count -gt 0) {
            $kernelArgs += @('--features', ($Features -join ','))
        }
        Invoke-Checked cargo $kernelArgs

        $sysroot = & rustc --print sysroot
        if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve Rust sysroot' }
        $objcopy = @(Get-ChildItem -LiteralPath (Join-Path $sysroot 'lib/rustlib') -Recurse -Filter llvm-objcopy.exe)
        if ($objcopy.Count -lt 1) { throw 'Install llvm-tools-preview: rustup component add llvm-tools-preview' }

        Copy-Item -LiteralPath 'boot/uefi/target/x86_64-unknown-uefi/release/aw-uefi-boot.efi' `
            -Destination (Join-Path $esp 'EFI/BOOT/BOOTX64.EFI') -Force
        Invoke-Checked $objcopy[0].FullName @('-O', 'binary',
            'kernel/x86_64/target/x86_64-unknown-none/release/aw-kernel-x86_64',
            (Join-Path $esp 'KERNEL.BIN'))

        # Keep the unstripped ELF next to the flat image for symbol-level triage.
        Copy-Item -LiteralPath 'kernel/x86_64/target/x86_64-unknown-none/release/aw-kernel-x86_64' `
            -Destination (Join-Path $run 'kernel.elf') -Force
    }

    if (-not (Test-Path -LiteralPath (Join-Path $esp 'KERNEL.BIN'))) {
        throw "No staged boot files for run '$Name'. Run without -NoBuild first."
    }

    $share = Join-Path (Split-Path $Qemu -Parent) 'share'
    $code = Join-Path $share 'edk2-x86_64-code.fd'
    $varsSrc = Join-Path $share 'edk2-i386-vars.fd'
    foreach ($required in @($Qemu, $code, $varsSrc)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing dependency: $required" }
    }
    $vars = Join-Path $run 'vars.fd'
    Copy-Item -LiteralPath $varsSrc -Destination $vars -Force

    $log = Join-Path $run 'debug.log'
    if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log -Force }

    $useMonitorScript = $MonitorScript.Count -gt 0
    $monitorBackend = if ($useMonitorScript) { 'stdio' } else { 'none' }

    $start = [System.Diagnostics.ProcessStartInfo]::new($Qemu)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardError = $true
    $start.RedirectStandardInput = $useMonitorScript
    $start.RedirectStandardOutput = $useMonitorScript
    $baseArgs = @(
        '-machine', 'q35', '-accel', 'tcg', '-cpu', 'max', '-m', '256M',
        '-display', 'none', '-serial', $Serial, '-monitor', $monitorBackend,
        '-net', 'none',
        '-debugcon', "file:$log",
        '-drive', "if=pflash,format=raw,readonly=on,file=$code",
        '-drive', "if=pflash,format=raw,file=$vars",
        '-drive', "format=raw,snapshot=on,file=fat:ro:$esp"
    )
    if (-not $AllowReboot) { $baseArgs += '-no-reboot' }
    foreach ($argument in ($baseArgs + $QemuArgs)) { $start.ArgumentList.Add($argument) }

    $process = [System.Diagnostics.Process]::Start($start)
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $stdoutTask = if ($useMonitorScript) { $process.StandardOutput.ReadToEndAsync() } else { $null }
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
    $exitedOnItsOwn = $false

    try {
        if ($useMonitorScript) {
            $logCursor = 0
            foreach ($step in $MonitorScript) {
                $parts = @($step -split '\|\|\|', 2)
                if ($parts.Count -ne 2 -or -not $parts[0] -or -not $parts[1]) {
                    throw "Invalid MonitorScript entry: $step"
                }
                $marker = $parts[0]
                $command = $parts[1]
                $seen = $false
                while (-not $seen) {
                    if ($process.HasExited) {
                        throw "QEMU exited before monitor marker appeared: $marker"
                    }
                    if ([DateTimeOffset]::UtcNow -ge $deadline) {
                        throw "Timed out waiting for monitor marker: $marker"
                    }
                    if (Test-Path -LiteralPath $log -PathType Leaf) {
                        $current = ((Get-Content -LiteralPath $log -Raw) -replace "`0", '')
                        if ($current.Length -lt $logCursor) { $logCursor = 0 }
                        if ($current.Length -gt $logCursor) {
                            $delta = $current.Substring($logCursor)
                            if ($delta.Contains($marker)) {
                                $logCursor = $current.Length
                                $seen = $true
                                break
                            }
                        }
                    }
                    Start-Sleep -Milliseconds 50
                }
                Write-Verbose "QEMU monitor after '$marker': $command"
                $process.StandardInput.WriteLine($command)
                $process.StandardInput.Flush()
            }
        }

        $remainingMs = [int]($deadline - [DateTimeOffset]::UtcNow).TotalMilliseconds
        if ($remainingMs -lt 1) { $remainingMs = 1 }
        $exitedOnItsOwn = $process.WaitForExit($remainingMs)
        if (-not $exitedOnItsOwn) {
            $process.Kill()
            $process.WaitForExit()
        }
    } finally {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        if ($useMonitorScript) {
            try { $process.StandardInput.Close() } catch {}
        }
    }
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $stdout = if ($stdoutTask) { $stdoutTask.GetAwaiter().GetResult() } else { '' }
    if ($stderr) { Write-Verbose "QEMU stderr: $stderr" }
    if ($stdout) { Write-Verbose "QEMU monitor: $stdout" }
    $process.Dispose()

    if (-not (Test-Path -LiteralPath $log)) { throw "QEMU produced no debug log for run '$Name'." }
    $text = ((Get-Content -LiteralPath $log -Raw) -replace "`0", '')

    [pscustomobject]@{
        Name    = $Name
        Log     = $log
        Text    = $text
        # True when QEMU stopped by itself (guest power off, or a reset under
        # -no-reboot) before the timeout, rather than being killed.
        ExitedOnItsOwn = $exitedOnItsOwn
        Markers = @($text -split "`r?`n" | Where-Object { $_ -match '^AW_' })
    }
} finally {
    Pop-Location
}
