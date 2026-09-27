# Complete the OMNI key with a data partition (Windows side of the project).
# The 256 MB EFI system partition (label OMNI) is never touched. In the free space
# after it, one exFAT partition "OMNI-DATA" is created once and then refreshed:
#   ST\         full ST release (compact + neural, private runtime), zip + sha256
#   NVDA\       stSynth NVDA add-on + instructions
#   UEFI\       copies of the EFI binaries on the ESP + run binding
#   BIOS\       read-only reference firmware image (never flashed by any tool here)
#   EVIDENCE\   backups of previous OMNI physical-boot evidence
#   LISEZMOI.TXT, SHA256SUMS.TXT
# Must run as NT AUTHORITY\SYSTEM:
#   C:\Tools\PsExec\PsExec64.exe -accepteula -s -h pwsh.exe -NoProfile -ExecutionPolicy Bypass -File tools\omni_key_data.ps1 ...
param(
    [Parameter(Mandatory)][string]$StRelease,
    [Parameter(Mandatory)][string]$NvdaAddon,
    [Parameter(Mandatory)][string]$BiosImage,
    [string]$EvidenceRoot = 'C:\OMNI-BACKUPS',
    [string]$ExpectedModel = 'USB DISK 3.2',
    [string]$EspLabel = 'OMNI',
    [string]$DataLabel = 'OMNI-DATA'
)
$ErrorActionPreference = 'Stop'
function Pass($m) { Write-Host "[PASS] $m" }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if ($identity.User.Value -ne 'S-1-5-18') { throw "must run as NT AUTHORITY\SYSTEM via PsExec64 -s (current: $($identity.Name))" }
Pass "identity $($identity.Name) (S-1-5-18)"
foreach ($p in $StRelease, $NvdaAddon, $BiosImage) { if (-not (Test-Path $p)) { throw "missing input: $p" } }

# Same physical identity rules as the EFI deployment.
$disks = @(Get-Disk | Where-Object { $_.FriendlyName.Trim() -eq $ExpectedModel -and $_.BusType -eq 'USB' })
if ($disks.Count -ne 1) { throw "expected exactly one USB disk '$ExpectedModel', found $($disks.Count)" }
$disk = $disks[0]
if ($disk.IsBoot -or $disk.IsSystem -or $disk.Size -lt 4GB -or $disk.Size -gt 64GB -or $disk.PartitionStyle -ne 'GPT') { throw "disk $($disk.Number) failed identity checks" }
$esp = @(Get-Partition -DiskNumber $disk.Number | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' })
if ($esp.Count -ne 1 -or ($esp[0] | Get-Volume).FileSystemLabel -ne $EspLabel) { throw "expected exactly one ESP labelled $EspLabel on disk $($disk.Number)" }
Pass "disk $($disk.Number) '$ExpectedModel' USB GPT, ESP '$EspLabel' present and left untouched"

$data = @(Get-Partition -DiskNumber $disk.Number | Where-Object { ($_ | Get-Volume -ErrorAction SilentlyContinue).FileSystemLabel -eq $DataLabel })
if ($data.Count -eq 0) {
    $others = @(Get-Partition -DiskNumber $disk.Number | Where-Object { $_.PartitionNumber -ne $esp[0].PartitionNumber })
    if ($others.Count -ne 0) { throw "unexpected extra partitions on disk $($disk.Number): refusing to create $DataLabel" }
    if ($disk.LargestFreeExtent -lt 2GB) { throw 'not enough free space for the data partition' }
    $part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -GptType '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}' -AssignDriveLetter
    if ($part.Offset -le $esp[0].Offset) { throw 'new partition is not after the ESP' }
    Format-Volume -Partition $part -FileSystem exFAT -NewFileSystemLabel $DataLabel -Confirm:$false | Out-Null
    $data = @(Get-Partition -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber)
    Pass "created partition $($part.PartitionNumber) exFAT '$DataLabel' ($([math]::Round($part.Size/1GB,1)) GB) in free space"
} elseif ($data.Count -ne 1) { throw "several '$DataLabel' partitions" }
if (-not $data[0].DriveLetter) { $data[0] | Add-PartitionAccessPath -AssignDriveLetter; $data = @(Get-Partition -DiskNumber $disk.Number -PartitionNumber $data[0].PartitionNumber) }
$root = "$($data[0].DriveLetter):\"
$espRoot = "$((Get-Partition -DiskNumber $disk.Number -PartitionNumber $esp[0].PartitionNumber).DriveLetter):\"

function Mirror($from, $to) {
    New-Item -ItemType Directory $to -Force | Out-Null
    robocopy $from $to /MIR /R:1 /W:1 /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy $from -> $to failed ($LASTEXITCODE)" }
}
Mirror $StRelease "$root\ST\$(Split-Path $StRelease -Leaf)"
foreach ($f in Get-ChildItem (Split-Path $StRelease) -Filter "$(Split-Path $StRelease -Leaf).zip*") { Copy-Item $f.FullName "$root\ST\" -Force }
Pass "ST release copied"
New-Item -ItemType Directory "$root\NVDA", "$root\BIOS", "$root\UEFI", "$root\EVIDENCE" -Force | Out-Null
Copy-Item $NvdaAddon, "$NvdaAddon.sha256" "$root\NVDA\" -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $StRelease '..\..\integrations\nvda\README.md') "$root\NVDA\README.md" -Force -ErrorAction SilentlyContinue
Copy-Item $BiosImage "$root\BIOS\" -Force
$biosSha = (Get-FileHash $BiosImage).Hash.ToLower()
@"
Image firmware ASUS de référence : $(Split-Path $BiosImage -Leaf)
SHA-256 : $biosSha
Usage : lecture seule (analyse HII / HDA). Aucun outil de ce projet ne la flashe.
Un flash BIOS reste une décision humaine, hors de ce dépôt.
"@ | Set-Content "$root\BIOS\LISEZMOI.TXT" -Encoding utf8
Pass "BIOS reference copied ($biosSha)"
Mirror "$espRoot\EFI" "$root\UEFI\EFI"
Copy-Item "$espRoot\OMNI-RUN-BINDING.JSON", "$espRoot\SHA256SUMS.TXT" "$root\UEFI\" -Force
Pass "UEFI binaries and binding mirrored from the ESP"
foreach ($b in Get-ChildItem $EvidenceRoot -Directory -Filter 'omni-usb-*') { Mirror $b.FullName "$root\EVIDENCE\$($b.Name)" }
Pass "previous OMNI evidence backups copied"

$binding = Get-Content "$espRoot\OMNI-RUN-BINDING.JSON" -Raw | ConvertFrom-Json
@"
CLÉ OMNI — CONTENU ET MODE D'EMPLOI

Partition OMNI (EFI, démarrable) :
  EFI\BOOT\BOOTX64.EFI      sonde OMNI : preuves, audio HDA, test clavier
  EFI\OMNI\SCREENREADER.EFI lecteur d'écran UEFI, lancé après la sonde
  OMNI-CHALLENGE.TXT, OMNI-RUN-BINDING.JSON, SHA256SUMS.TXT

Partition OMNI-DATA (cette partition) :
  ST\        synthèse vocale ST $((Get-Content (Join-Path $StRelease 'MANIFEST.json') -Raw | ConvertFrom-Json).version) pour Windows (st.exe, st_synth.dll, voix neuronales)
  NVDA\      add-on NVDA « ST » (ouvrir le fichier .nvda-addon pour l'installer)
  UEFI\      copie des binaires EFI et de leur liaison au build
  BIOS\      image firmware de référence, lecture seule, jamais flashée
  EVIDENCE\  preuves des démarrages physiques précédents

Démarrage physique (run $($binding.runId), commit $($binding.commit.Substring(0,7))) :
  1. Redémarrer : le PC démarre une fois sur la clé (BootNext).
  2. Écouter le haut-parleur : une tonalité doit sortir.
  3. Au message INPUT TEST : Flèche bas puis Entrée dans les 30 secondes.
  4. Le lecteur d'écran UEFI démarre : naviguer avec les flèches.
  5. Revenir à Windows puis lancer la collecte des preuves
     (tools\omni_collect_physical.ps1 sous PsExec64 -s).
"@ | Set-Content "$root\LISEZMOI.TXT" -Encoding utf8

Get-ChildItem $root -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.TXT' } | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName).Hash.ToLower(), $_.FullName.Substring($root.Length).Replace('\', '/')
} | Set-Content "$root\SHA256SUMS.TXT" -Encoding utf8
$vol = Get-Volume -DriveLetter $data[0].DriveLetter
Pass ("{0} complete: {1:N0} files, {2:N1} GB used of {3:N1} GB" -f $DataLabel, (Get-ChildItem $root -Recurse -File).Count, (($vol.Size - $vol.SizeRemaining) / 1GB), ($vol.Size / 1GB))
exit 0  # robocopy leaves $LASTEXITCODE=1 on success
