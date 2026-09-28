# Inject ST (neural voice + SAPI 5 engine) into the mounted WinRE copy. Run as SYSTEM.
param([string]$Mount = 'C:\OMNI-BACKUPS\winre\mount', [string]$Source = 'C:\st\work\sapi-test', [string]$Log)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path $Log -Force | Out-Null
try {
    if (-not (Test-Path "$Mount\Windows\System32\winpeshl.ini")) { throw "WinRE image not mounted at $Mount" }
    $st = Join-Path $Mount 'Program Files\ST'
    New-Item -ItemType Directory -Force $st | Out-Null
    Copy-Item "$Source\*" $st -Recurse -Force -Exclude '*.wav'
    foreach ($f in 'st_synth.dll', 'st_sapi.dll', 'neural\worker.py', 'neural\python\python.exe') {
        if (-not (Test-Path (Join-Path $st $f))) { throw "missing $f" }
    }
    $mb = [math]::Round((Get-ChildItem $st -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)
    Write-Host "[PASS] ST copied into image: $mb MB"
} finally { Stop-Transcript | Out-Null }
