#requires -Version 7.0
#requires -RunAsAdministrator

[CmdletBinding()]
param(
    [string]$RunnerDir = 'C:\actions-runner-accessible-windows',
    [string]$ExpectedRunnerName = 'AMD-5800H-REAL'
)

$ErrorActionPreference = 'Stop'

function Write-Gate {
    param([string]$Name, [bool]$Pass, [string]$Detail = '')
    $state = if ($Pass) { 'PASS' } else { 'FAIL' }
    if ($Detail) {
        Write-Host ("{0} = {1} :: {2}" -f $Name, $state, $Detail)
    } else {
        Write-Host ("{0} = {1}" -f $Name, $state)
    }
}

function Get-RunnerService {
    param([string]$Root)

    # On Windows, GitHub stores the configured service name in .service.
    # Prefer that authoritative mapping before falling back to executable-path discovery.
    $serviceFile = Join-Path $Root '.service'
    if (Test-Path $serviceFile) {
        $serviceName = (Get-Content $serviceFile -Raw -ErrorAction SilentlyContinue).Trim()
        if ($serviceName) {
            $service = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -eq $serviceName } |
                Select-Object -First 1
            if ($service) {
                return $service
            }
        }
    }

    Get-CimInstance Win32_Service -ErrorAction SilentlyContinue |
        Where-Object {
            $_.PathName -match 'RunnerService\.exe' -and
            $_.PathName -like "*$Root*"
        } |
        Select-Object -First 1
}

function Get-RunnerListener {
    param([string]$Root)
    Get-CimInstance Win32_Process |
        Where-Object {
            $_.Name -eq 'Runner.Listener.exe' -and
            $_.ExecutablePath -like "$Root*"
        }
}

function Show-Diagnostics {
    param([string]$Root, [string]$ServiceName)

    Write-Host ''
    Write-Host '=== RUNNER LOG ==='
    $diag = Join-Path $Root '_diag'
    if (Test-Path $diag) {
        $latest = Get-ChildItem $diag -File -Filter 'Runner_*.log' -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if ($latest) {
            Write-Host ("LOG = {0}" -f $latest.FullName)
            Get-Content $latest.FullName -Tail 160
        } else {
            Write-Host 'Runner_*.log absent.'
        }
    } else {
        Write-Host '_diag absent.'
    }

    Write-Host ''
    Write-Host '=== SERVICE CONTROL MANAGER ==='
    Get-WinEvent -FilterHashtable @{
        LogName = 'System'
        StartTime = (Get-Date).AddMinutes(-30)
    } -ErrorAction SilentlyContinue |
        Where-Object {
            $_.ProviderName -eq 'Service Control Manager' -and
            ($_.Message -like "*$ServiceName*" -or $_.Id -in 7000,7001,7009,7011,7023,7024,7031,7034)
        } |
        Select-Object -First 20 TimeCreated, Id, LevelDisplayName, Message |
        Format-List
}

Write-Host '=== AMD-5800H-REAL RUNNER RECOVERY ==='

if (-not (Test-Path $RunnerDir)) {
    $candidate = Get-ChildItem 'C:\' -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'actions-runner-*accessible*windows*' } |
        Select-Object -First 1
    if (-not $candidate) {
        throw "Runner directory not found: $RunnerDir"
    }
    $RunnerDir = $candidate.FullName
}

$RunnerDir = (Resolve-Path $RunnerDir).Path
Set-Location $RunnerDir
Write-Host ("RUNNER_DIR = {0}" -f $RunnerDir)

$required = @(
    '.runner',
    'bin\RunnerService.exe',
    'bin\Runner.Listener.exe'
)

foreach ($item in $required) {
    $path = Join-Path $RunnerDir $item
    if (-not (Test-Path $path)) {
        throw "Required runner file missing: $path"
    }
}
Write-Gate 'RUNNER_FILES' $true

try {
    $cfg = Get-Content (Join-Path $RunnerDir '.runner') -Raw | ConvertFrom-Json
    $configuredName = [string]$cfg.agentName
    if ($configuredName) {
        Write-Host ("CONFIGURED_RUNNER_NAME = {0}" -f $configuredName)
        if ($configuredName -ne $ExpectedRunnerName) {
            throw "Runner identity mismatch: expected '$ExpectedRunnerName', got '$configuredName'"
        }
        Write-Gate 'RUNNER_IDENTITY' $true $configuredName
    }
} catch {
    if ($_.Exception.Message -like 'Runner identity mismatch:*') { throw }
    Write-Host ("RUNNER_CONFIG_PARSE = WARNING :: {0}" -f $_.Exception.Message)
}

$service = Get-RunnerService $RunnerDir

if (-not $service) {
    Write-Gate 'WINDOWS_SERVICE_CONFIGURED' $false
    throw @"
GitHub Actions Runner Windows service is not installed for this runner.
This is not a missing svc.cmd problem: Windows runners do not use svc.cmd.
Reconfigure the runner with config.cmd and choose service mode, then rerun this recovery script.
"@
}

Write-Gate 'WINDOWS_SERVICE_CONFIGURED' $true $service.Name

Write-Host ("SERVICE = {0}" -f $service.Name)
Write-Host ("SERVICE_ACCOUNT = {0}" -f $service.StartName)
Write-Host ("SERVICE_STATE_BEFORE = {0}" -f $service.State)
Write-Host ("SERVICE_STARTMODE_BEFORE = {0}" -f $service.StartMode)

# A manually launched listener can prevent the service listener from owning the runner session.
if ($service.State -ne 'Running') {
    $manual = @(Get-RunnerListener $RunnerDir)
    foreach ($proc in $manual) {
        Write-Host ("STOP_MANUAL_LISTENER_PID = {0}" -f $proc.ProcessId)
        Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if ($manual.Count -gt 0) {
        Start-Sleep -Seconds 2
    }
}

Set-Service -Name $service.Name -StartupType Automatic

# Restart the service automatically if the listener/service process fails.
& sc.exe failure $service.Name reset= 0 actions= restart/5000/restart/5000/restart/5000 | Out-Host
& sc.exe failureflag $service.Name 1 | Out-Host

try {
    $serviceNow = Get-Service -Name $service.Name -ErrorAction Stop
    if ($serviceNow.Status -ne 'Running') {
        Start-Service -Name $service.Name -ErrorAction Stop
    }
} catch {
    Write-Host ("SERVICE_START_EXCEPTION = {0}" -f $_.Exception.Message)
}

$deadline = (Get-Date).AddSeconds(20)
do {
    Start-Sleep -Seconds 2
    $service = Get-CimInstance Win32_Service -Filter "Name='$($service.Name)'"
    $listener = @(Get-RunnerListener $RunnerDir)
    if ($service.State -eq 'Running' -and $listener.Count -gt 0) {
        break
    }
} while ((Get-Date) -lt $deadline)

$service = Get-CimInstance Win32_Service -Filter "Name='$($service.Name)'"
$listener = @(Get-RunnerListener $RunnerDir)

Write-Host ''
Write-Host '=== FINAL STATE ==='
Write-Host ("STATE = {0}" -f $service.State)
Write-Host ("START = {0}" -f $service.StartMode)
Write-Host ("EXITCODE = {0}" -f $service.ExitCode)
Write-Host ("SERVICE_EXITCODE = {0}" -f $service.ServiceSpecificExitCode)
Write-Host ("LISTENER_COUNT = {0}" -f $listener.Count)

if ($listener.Count -gt 0) {
    $listener |
        Select-Object ProcessId, Name, ExecutablePath, CommandLine |
        Format-List
}

$permanent = (
    $service.State -eq 'Running' -and
    $service.StartMode -eq 'Auto' -and
    $listener.Count -gt 0
)

Write-Host ''
if ($permanent) {
    Write-Gate 'RUNNER_AMD_5800H_REAL_PERMANENT' $true
    Write-Gate 'WINDOWS_AUTOSTART' $true
    Write-Gate 'LISTENER_SERVICE_OWNED_RUNTIME' $true
    exit 0
}

Write-Gate 'RUNNER_AMD_5800H_REAL_PERMANENT' $false ("State={0}; Start={1}; ListenerCount={2}" -f $service.State, $service.StartMode, $listener.Count)
Show-Diagnostics -Root $RunnerDir -ServiceName $service.Name
exit 1
