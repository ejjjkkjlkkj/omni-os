# Attach the VM disk read-only, copy \diag to a new timestamped folder, detach. Run as SYSTEM.
param([string]$Vhd = 'C:\OMNI-BACKUPS\winre\winre-vm.vhd',
      [string]$Out = ('C:\OMNI-BACKUPS\winre\diag-' + (Get-Date -Format 'yyyyMMdd-HHmmss')), [string]$Log)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path $Log -Force | Out-Null
try {
    Mount-DiskImage -ImagePath $Vhd -Access ReadOnly | Out-Null
    $n = (Get-DiskImage -ImagePath $Vhd).Number
    foreach ($q in Get-Partition -DiskNumber $n) { if (-not $q.DriveLetter) { $q | Add-PartitionAccessPath -AssignDriveLetter } }
    $p = Get-Partition -DiskNumber $n | Where-Object { $_.DriveLetter -and (Test-Path "$($_.DriveLetter):\diag") } | Select-Object -First 1
    if (-not $p) { throw "no \diag on $Vhd" }
    Copy-Item "$($p.DriveLetter):\diag" $Out -Recurse
    Set-Content 'C:\OMNI-BACKUPS\winre\diag-latest.txt' $Out
    Write-Host "[PASS] diag copied from $($p.DriveLetter): to $Out"
} finally {
    Dismount-DiskImage -ImagePath $Vhd | Out-Null
    Stop-Transcript | Out-Null
}
