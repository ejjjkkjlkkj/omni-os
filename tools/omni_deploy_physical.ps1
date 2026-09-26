# Deploy a HIL-built OmniProbe to the physical OMNI USB key and arm BootNext.
# Stops BEFORE rebooting: the reboot, the listening test and the key presses are
# done by a person at the machine. Run as NT AUTHORITY\SYSTEM (PsExec64 -s) or
# an elevated administrator.
#
#   pwsh -File tools\omni_deploy_physical.ps1 -RunId <HIL run id> [-WhatIf]
[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)][long]$RunId,
    [string]$Repo = 'ejjjkkjlkkj/solution',
    [string]$Artifact = 'omni-uefi-hil',
    [string]$BackupRoot = 'C:\OMNI-BACKUPS',
    [string]$ExpectedModel = 'USB DISK 3.2',
    [string]$Label = 'OMNI'
)
$ErrorActionPreference = 'Stop'
function Pass($m) { Write-Host "[PASS] $m" }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = (New-Object Security.Principal.WindowsPrincipal $identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not ($isAdmin -or $identity.User.Value -eq 'S-1-5-18')) { throw "needs SYSTEM or elevated administrator (current: $($identity.Name))" }
Pass "identity $($identity.Name) ($($identity.User.Value))"

# 1. Physical identity of the key: exactly one match, never boot/system.
$disks = @(Get-Disk | Where-Object { $_.FriendlyName.Trim() -eq $ExpectedModel -and $_.BusType -eq 'USB' })
if ($disks.Count -ne 1) { throw "expected exactly one USB disk '$ExpectedModel', found $($disks.Count)" }
$disk = $disks[0]
if ($disk.IsBoot -or $disk.IsSystem) { throw "disk $($disk.Number) is boot/system: refusing" }
if ($disk.Size -lt 4GB -or $disk.Size -gt 64GB) { throw "unexpected size $($disk.Size)" }
$vols = @(Get-Partition -DiskNumber $disk.Number | Get-Volume | Where-Object { $_.FileSystemLabel -eq $Label -and $_.FileSystem -eq 'FAT32' })
if ($vols.Count -ne 1 -or -not $vols[0].DriveLetter) { throw "expected one FAT32 '$Label' volume with a drive letter on disk $($disk.Number)" }
$root = "$($vols[0].DriveLetter):\"
Pass "disk $($disk.Number) '$($disk.FriendlyName.Trim())' USB $([math]::Round($disk.Size/1GB,1)) GB, boot=False system=False, volume $root label=$Label"

# 2. Artifact from the named HIL run, integrity checked against its manifest.
$run = gh run view $RunId --repo $Repo --json conclusion,headSha,headBranch,workflowName | ConvertFrom-Json
if ($run.workflowName -ne 'Physical AMD HIL' -or $run.conclusion -ne 'success') { throw "run $RunId is not a successful Physical AMD HIL run ($($run.workflowName) / $($run.conclusion))" }
$stage = Join-Path $BackupRoot "stage-$RunId-$stamp"
gh run download $RunId --repo $Repo --name $Artifact --dir $stage
if ($LASTEXITCODE) { throw 'artifact download failed' }
$efi = Get-ChildItem $stage -Recurse -Include OmniProbe.efi, BOOTX64.EFI | Select-Object -First 1
if (-not $efi) { throw 'OmniProbe.efi / BOOTX64.EFI not found in artifact' }
$efiSha = (Get-FileHash $efi.FullName -Algorithm SHA256).Hash.ToLower()
$listed = Select-String -Path (Get-ChildItem $stage -Recurse -File | Where-Object { $_.Extension -in '.json', '.txt' }).FullName -Pattern $efiSha -SimpleMatch -List
if (-not $listed) { throw "EFI SHA-256 $efiSha is not listed in any artifact manifest" }
Pass "artifact EFI $efiSha (commit $($run.headSha), branch $($run.headBranch)) listed in $(Split-Path $listed[0].Path -Leaf)"

# 3. Back up the whole key, then remove evidence of the previous boot so a failed
#    boot can never be mistaken for a new proof.
$backup = Join-Path $BackupRoot "omni-usb-$stamp"
if ($PSCmdlet.ShouldProcess($root, "back up to $backup")) {
    New-Item -ItemType Directory $backup | Out-Null
    Copy-Item "$root*" $backup -Recurse -Force
    Get-ChildItem $backup -Recurse -File | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName).Hash.ToLower(), $_.FullName.Substring($backup.Length + 1) } | Set-Content "$backup\BACKUP-SHA256.TXT"
    Pass "backup $backup ($((Get-ChildItem $backup -Recurse -File).Count) files)"
}
foreach ($stale in 'OMNI-EVIDENCE.TXT', 'OMNI-DIAG.TXT', 'OMNI-TRACE.TXT', 'OMNI-PREPARED.TXT', 'OMNI-RUN-BINDING.JSON') {
    if ((Test-Path "$root$stale") -and $PSCmdlet.ShouldProcess("$root$stale", 'remove (backed up)')) { Remove-Item "$root$stale" -Force }
}

# 4. Install the EFI, fresh challenge (kept off the media too), run binding.
$challengeOut = Join-Path $BackupRoot "challenge-$RunId-$stamp.txt"
if ($PSCmdlet.ShouldProcess("$root\EFI\BOOT\BOOTX64.EFI", 'install')) {
    New-Item -ItemType Directory "$root\EFI\BOOT" -Force | Out-Null
    Copy-Item $efi.FullName "$root\EFI\BOOT\BOOTX64.EFI" -Force
    $solution = Resolve-Path "$PSScriptRoot\.."
    $env:PYTHONPATH = $solution
    $prep = python -m tools.prepare_physical_media --mount $root --expected-sha256 $efiSha --challenge-out $challengeOut | ConvertFrom-Json
    if ($prep.status -ne 'PHYSICAL_MEDIA_PREPARED') { throw "prepare failed: $($prep.error)" }
    [ordered]@{ schema = 'omni.run-binding.v1'; runId = $RunId; repository = $Repo; branch = $run.headBranch; commit = $run.headSha
        artifact = $Artifact; efiSha256 = $efiSha; challenge = $prep.challenge; preparedUtc = (Get-Date).ToUniversalTime().ToString('o')
        preparedBy = $identity.Name; machine = $env:COMPUTERNAME } | ConvertTo-Json | Set-Content "$root\OMNI-RUN-BINDING.JSON" -Encoding ascii
    Pass "EFI installed and verified on media, challenge $($prep.challenge)"
}

# 5. BootNext = the firmware entry of this USB key (one-shot; normal boot order untouched).
$fw = bcdedit /enum firmware | Out-String
# bcdedit localizes the key ("identifier" / "identificateur"); match both.
$entries = [regex]::Matches($fw, '(?ms)^(?:identifier|identificateur)\s+(\{[0-9a-f-]+\})\s*$(.*?)(?=^(?:identifier|identificateur)|\z)')
$usb = @($entries | Where-Object { $_.Groups[2].Value -match [regex]::Escape($ExpectedModel) -and $_.Groups[2].Value -match "partition=$($vols[0].DriveLetter):" })
if ($usb.Count -ne 1) { throw "expected exactly one USB firmware boot entry, found $($usb.Count); set BootNext by hand" }
$guid = $usb[0].Groups[1].Value
if ($PSCmdlet.ShouldProcess($guid, 'bcdedit /set {fwbootmgr} bootsequence')) {
    bcdedit /set '{fwbootmgr}' bootsequence $guid | Out-Null
    if ($LASTEXITCODE) { throw 'bcdedit bootsequence failed' }
    Pass "BootNext (one-shot) = $guid"
}
Write-Host ""
Write-Host "READY. Nothing has been rebooted. At the machine: restart, listen to the speaker,"
Write-Host "press keys when OmniProbe asks, let it return to Windows, then collect D:\OMNI-*.TXT."
