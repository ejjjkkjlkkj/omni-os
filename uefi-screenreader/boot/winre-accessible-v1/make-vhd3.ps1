# Full-chain VM disk, laid out like the future OMNI key:
#   partition 1 (FAT32): \EFI\BOOT\BOOTX64.EFI = UEFI screen reader, \EFI\OMNI\NAV.BIN
#   partition 2 (FAT32): Windows boot manager + BCD ("boot" devices) + boot.sdi + sources\winre.wim
# Run as SYSTEM.
param([string]$Vhd = 'C:\OMNI-BACKUPS\winre\chain-vm.vhd', [Parameter(Mandatory)][string]$Efi,
      [Parameter(Mandatory)][string]$Nav, [string]$Wim = 'C:\OMNI-BACKUPS\winre\winre-accessible.wim', [string]$Log)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path $Log -Force | Out-Null
$attached = $false
try {
    if (Test-Path $Vhd) { [IO.File]::Delete($Vhd) }
    $size = [math]::Ceiling(((Get-Item $Wim).Length + (Get-Item $Nav).Length) / 1MB) + 700
    $dp = [IO.Path]::GetTempFileName()
    @("create vdisk file=`"$Vhd`" maximum=$size type=fixed", "select vdisk file=`"$Vhd`"", 'attach vdisk', 'convert mbr',
      "create partition primary size=$([math]::Max(300, [math]::Ceiling((Get-Item $Nav).Length / 1MB) + 64))",
      'format fs=fat32 quick label=OMNI', 'assign',
      'create partition primary', 'format fs=fat32 quick label=OMNI-WINRE', 'assign') | Set-Content $dp -Encoding ascii
    diskpart /s $dp | Out-Null
    $attached = $true
    $n = (Get-DiskImage -ImagePath $Vhd).Number
    $parts = Get-Partition -DiskNumber $n | Where-Object DriveLetter | Sort-Object Offset
    $p1 = "$($parts[0].DriveLetter):"; $p2 = "$($parts[1].DriveLetter):"
    New-Item -ItemType Directory -Force "$p1\EFI\BOOT", "$p1\EFI\OMNI", "$p2\EFI\Microsoft\Boot", "$p2\boot", "$p2\sources" | Out-Null
    Copy-Item $Efi "$p1\EFI\BOOT\BOOTX64.EFI"
    Copy-Item $Nav "$p1\EFI\OMNI\NAV.BIN"
    Copy-Item C:\Windows\Boot\EFI\bootmgfw.efi "$p2\EFI\Microsoft\Boot\bootmgfw.efi"
    Copy-Item C:\Windows\Boot\DVD\EFI\boot.sdi "$p2\boot\boot.sdi"
    Copy-Item $Wim "$p2\sources\winre.wim"
    & C:\OMNI-BACKUPS\winre\new-winre-bcd.ps1 -Store "$p2\EFI\Microsoft\Boot\BCD"
    Write-Host "[PASS] chain VHD: reader on $p1, WinRE on $p2"
} finally {
    if ($attached) {
        $dp2 = [IO.Path]::GetTempFileName()
        @("select vdisk file=`"$Vhd`"", 'detach vdisk') | Set-Content $dp2 -Encoding ascii
        diskpart /s $dp2 | Out-Null
        Write-Host '[PASS] chain VHD detached'
    }
    Stop-Transcript | Out-Null
}
