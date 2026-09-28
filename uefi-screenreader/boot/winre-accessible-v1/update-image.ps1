# Add files to the accessible WinRE copy and set its launch sequence. Run as SYSTEM.
param([string]$Wim = 'C:\OMNI-BACKUPS\winre\winre-accessible.wim', [string]$Mount = 'C:\OMNI-BACKUPS\winre\mount',
      [string[]]$Add = @(), [string[]]$Launch = @(), [string]$Log)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path $Log -Force | Out-Null
$mounted = $false
try {
    New-Item -ItemType Directory -Force $Mount | Out-Null
    dism /English /Mount-Image /ImageFile:$Wim /Index:1 /MountDir:$Mount | Select-Object -Last 1
    if ($LASTEXITCODE) { throw 'mount failed' }
    $mounted = $true
    foreach ($pair in $Add) {
        $src, $dst = $pair -split '=>', 2
        $target = Join-Path $Mount $dst
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        Copy-Item $src $target -Force
        Write-Host "[PASS] added $dst"
    }
    if ($Launch) {
        (@('[LaunchApps]') + $Launch) | Set-Content "$Mount\Windows\System32\winpeshl.ini" -Encoding ascii
        Get-Content "$Mount\Windows\System32\winpeshl.ini" | Out-Host
    }
    dism /English /Unmount-Image /MountDir:$Mount /Commit | Select-Object -Last 1
    if ($LASTEXITCODE) { throw 'commit failed' }
    $mounted = $false
    Write-Host "[PASS] committed $((Get-FileHash $Wim).Hash)"
} finally {
    if ($mounted) { dism /English /Unmount-Image /MountDir:$Mount /Discard | Out-Null; Write-Host 'discarded' }
    Stop-Transcript | Out-Null
}
