# OMNI Local Runner
# Safe orchestration entrypoint.
# Default mode is read-only. USB/UEFI writes require explicit switches.

[CmdletBinding()]
param(
    [switch]$RunTests,
    [switch]$RunGitStatus,
    [switch]$AllowUsbWrite,
    [switch]$AllowReboot
)

$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot
$LogDir = Join-Path $Repo 'logs'
$LogFile = Join-Path $LogDir ("omni-run-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
Start-Transcript -Path $LogFile -Force | Out-Null

try {
    Write-Host '=== OMNI LOCAL RUNNER ===' -ForegroundColor Cyan
    Write-Host "Repo: $Repo"
    Write-Host "Time: $(Get-Date -Format o)"

    Write-Host ''
    Write-Host '[1] Environment' -ForegroundColor Cyan
    Write-Host "PowerShell: $($PSVersionTable.PSVersion)"
    if ($PSVersionTable.PSVersion -lt [version]'7.6.6') {
        throw 'PowerShell 7.6.6+ requis.'
    }

    $Git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $Git) { throw 'Git introuvable.' }
    Write-Host "Git: $($Git.Source)"

    $PsExec = Join-Path $env:SystemRoot 'System32\PsExec64.exe'
    if (Test-Path -LiteralPath $PsExec -PathType Leaf) {
        Write-Host "PsExec: $PsExec"
    } else {
        Write-Host 'PsExec: absent (non bloquant en mode lecture seule).' -ForegroundColor Yellow
    }

    Write-Host ''
    Write-Host '[2] Repository' -ForegroundColor Cyan
    Push-Location $Repo
    try {
        $branch = git branch --show-current
        $head = git rev-parse HEAD
        Write-Host "Branch: $branch"
        Write-Host "HEAD:   $head"

        if ($RunGitStatus) {
            Write-Host ''
            git status --short
        }
    }
    finally {
        Pop-Location
    }

    Write-Host ''
    Write-Host '[3] Hardware inventory (read-only)' -ForegroundColor Cyan
    $disks = @(Get-Disk | Select-Object Number,FriendlyName,BusType,PartitionStyle,OperationalStatus,IsReadOnly,Size)
    $disks | Format-Table -AutoSize

    $usb = @($disks | Where-Object BusType -eq 'USB')
    Write-Host "USB disks detected: $($usb.Count)"

    if ($AllowUsbWrite) {
        Write-Host ''
        Write-Host 'USB WRITE AUTHORIZATION DETECTED.' -ForegroundColor Yellow
        Write-Host 'This runner does not format, erase, partition, or write a USB by itself.'
    } else {
        Write-Host 'USB write operations: BLOCKED (default).' -ForegroundColor Green
    }

    if ($AllowReboot) {
        Write-Host 'Reboot authorization supplied, but this runner will not reboot automatically.' -ForegroundColor Yellow
    } else {
        Write-Host 'Reboot: BLOCKED (default).' -ForegroundColor Green
    }

    if ($RunTests) {
        Write-Host ''
        Write-Host '[4] Tests' -ForegroundColor Cyan
        $Python = Get-Command python -ErrorAction SilentlyContinue
        if (-not $Python) { throw 'Python introuvable.' }
        Push-Location $Repo
        try {
            $env:PYTHONPATH = Join-Path $Repo 'src'
            & $Python.Source -m unittest discover -s tests -v
            if ($LASTEXITCODE -ne 0) {
                throw "unittest failed with exit code $LASTEXITCODE."
            }
        }
        finally {
            Pop-Location
        }
    } else {
        Write-Host ''
        Write-Host '[4] Tests skipped. Use -RunTests to execute them.'
    }

    Write-Host ''
    Write-Host '=== RUN COMPLETE ===' -ForegroundColor Green
    Write-Host "Log: $LogFile"
}
catch {
    Write-Host ''
    Write-Host '=== RUN FAILED ===' -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    Write-Host "Log: $LogFile"
    exit 1
}
finally {
    Stop-Transcript | Out-Null
}
