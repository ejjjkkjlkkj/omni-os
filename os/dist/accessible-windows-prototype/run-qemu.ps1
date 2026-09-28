#Requires -Version 7.0
# Lance le prototype Accessible Windows sous QEMU/OVMF depuis ce dossier.
[CmdletBinding()]
param(
    [string]$Qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe',
    [int]$TimeoutSeconds = 20,
    [switch]$ShowWindow
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$esp  = Join-Path $here 'esp'
if (-not (Test-Path -LiteralPath $Qemu)) { throw "QEMU introuvable: $Qemu (passez -Qemu <chemin>)" }
$share = Join-Path (Split-Path $Qemu -Parent) 'share'
$code  = Join-Path $share 'edk2-x86_64-code.fd'
$varsSrc = Join-Path $share 'edk2-i386-vars.fd'
foreach ($f in @($code, $varsSrc)) {
    if (-not (Test-Path -LiteralPath $f)) { throw "Firmware EDK2 manquant: $f" }
}
$vars = Join-Path $here 'vars.fd'
Copy-Item -LiteralPath $varsSrc -Destination $vars -Force
$log = Join-Path $here 'debug.log'
$qargs = [System.Collections.Generic.List[string]]::new()
foreach ($a in @(
    '-machine','q35','-accel','tcg','-cpu','max','-m','256M',
    '-no-reboot','-net','none','-monitor','none','-serial','none',
    '-debugcon',"file:$log",
    '-drive',"if=pflash,format=raw,readonly=on,file=$code",
    '-drive',"if=pflash,format=raw,file=$vars",
    '-drive',"format=raw,snapshot=on,file=fat:ro:$esp")) { $qargs.Add($a) }
if (-not $ShowWindow) { $qargs.Add('-display'); $qargs.Add('none') }

Write-Host "Demarrage du prototype sous QEMU (timeout ${TimeoutSeconds}s)..."
$si = [System.Diagnostics.ProcessStartInfo]::new($Qemu)
$si.UseShellExecute = $false
$si.RedirectStandardError = $true
if (-not $ShowWindow) { $si.CreateNoWindow = $true }
foreach ($a in $qargs) { $si.ArgumentList.Add($a) }
$p = [System.Diagnostics.Process]::Start($si)
$err = $p.StandardError.ReadToEndAsync()
if (-not $p.WaitForExit($TimeoutSeconds * 1000)) { $p.Kill(); $p.WaitForExit() }
$stderr = $err.GetAwaiter().GetResult()
if ($stderr) { Write-Host "QEMU stderr:`n$stderr" }

Write-Host "Journal de diagnostic : $log"
if (Test-Path -LiteralPath $log) {
    Write-Host "Marqueurs de demarrage :"
    ((Get-Content -LiteralPath $log -Raw) -replace "`0",'') -split "`n" |
        Where-Object { $_ -match '^AW_' } | ForEach-Object { Write-Host "  $_" }
}
