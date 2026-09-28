# Two-stage OMNI deployment with the privileged part under NT AUTHORITY\SYSTEM.
#   1. current user: download the HIL artifact and its run metadata (gh auth stays here)
#   2. PsExec64 -s: tools\omni_deploy_physical.ps1 -RequireSystem (disk checks, backup,
#      key writes, BootNext). Never reboots.
#
#   pwsh -File tools\omni_deploy_system.ps1 -RunId <HIL run> -ScreenReader <REALTIME.EFI>
param(
    [Parameter(Mandatory)][long]$RunId,
    [string]$ScreenReader,
    [string]$Repo = 'ejjjkkjlkkj/solution',
    [string]$PsExec = 'C:\Tools\PsExec\PsExec64.exe',
    [string]$BackupRoot = 'C:\OMNI-BACKUPS'
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $PsExec -PathType Leaf)) { throw "PsExec64 not found: $PsExec" }
Write-Host "[PASS] PsExec64=$PsExec"

$stage = Join-Path $BackupRoot ("stage-{0}-{1}" -f $RunId, (Get-Date -Format 'yyyyMMdd-HHmmss'))
gh run download $RunId --repo $Repo --name omni-uefi-hil --dir $stage
if ($LASTEXITCODE) { throw 'artifact download failed' }
$run = gh run view $RunId --repo $Repo --json databaseId,conclusion,headSha,headBranch,workflowName
if ($LASTEXITCODE) { throw 'run metadata failed' }
Set-Content (Join-Path $stage 'run.json') $run -Encoding utf8
Write-Host "[PASS] artifact and run metadata staged in $stage"

$script = Join-Path $PSScriptRoot 'omni_deploy_physical.ps1'
$log = Join-Path $stage 'deploy-system.log'
$args = @('-accepteula', '-nobanner', '-s', '-h', '-w', (Resolve-Path "$PSScriptRoot\.."), 'pwsh.exe', '-NoLogo', '-NoProfile',
          '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command',
          "& '$script' -RunId $RunId -ArtifactDir '$stage' -RequireSystem $(if ($ScreenReader) { "-ScreenReader '$((Resolve-Path $ScreenReader).Path)'" }) *>&1 | Tee-Object -FilePath '$log'; exit `$LASTEXITCODE")
& $PsExec @args
$code = $LASTEXITCODE
Get-Content $log -ErrorAction SilentlyContinue | Where-Object { $_ -notmatch 'alias' }
if ($code -ne 0) { throw "SYSTEM deployment failed (PsExec exit $code), see $log" }
Write-Host "[PASS] deployed under NT AUTHORITY\SYSTEM"
