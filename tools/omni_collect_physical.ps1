# After the physical OMNI boot: copy the evidence off the key, verify it fail-closed
# against the run binding written by omni_deploy_physical.ps1, and compare the HDA
# gates with docs/HDA_ANALOG_ROUTE.md. Read-only on the key.
#
#   C:\Tools\PsExec\PsExec64.exe -accepteula -s -h -w <repo> pwsh.exe -NoProfile -ExecutionPolicy Bypass `n#       -File tools\omni_collect_physical.ps1 [-Label OMNI] [-PlatformUuid bb58f448-...]
param(
    [string]$Label = 'OMNI',
    [string]$OutRoot = 'C:\OMNI-PHYSICAL-EVIDENCE',
    [string]$PlatformUuid = 'bb58f448-c083-e24c-acfb-00153fe8bb5a'
)
$ErrorActionPreference = 'Stop'
$vol = @(Get-Volume | Where-Object { $_.FileSystemLabel -eq $Label -and $_.DriveLetter })
if ($vol.Count -ne 1) { throw "expected one '$Label' volume" }
$root = "$($vol[0].DriveLetter):\"
$binding = Get-Content "$root\OMNI-RUN-BINDING.JSON" -Raw | ConvertFrom-Json
$out = Join-Path $OutRoot ("run-{0}-{1}" -f $binding.runId, (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory $out | Out-Null
Copy-Item "$root*" $out -Recurse -Force
Get-ChildItem $out -Recurse -File | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName).Hash.ToLower(), $_.FullName.Substring($out.Length + 1) } | Set-Content "$out\COLLECTED-SHA256.TXT"
Write-Host "[PASS] evidence copied to $out"

$env:PYTHONPATH = Resolve-Path "$PSScriptRoot\.."
$verdict = [ordered]@{ run = $binding.runId; commit = $binding.commit; efiSha256 = $binding.efiSha256 }
if (Test-Path "$out\OMNI-EVIDENCE.TXT") {
    $py = (Get-Command python -ErrorAction SilentlyContinue).Source
    if (-not $py) { $py = 'C:\Program Files\Python313\python.exe' }
    $v = & $py -m tools.verify_uefi_evidence --evidence "$out\OMNI-EVIDENCE.TXT" --efi "$out\EFI\BOOT\BOOTX64.EFI" `
        --expected-sha256 $binding.efiSha256 --expected-challenge $binding.challenge --expected-platform-uuid $PlatformUuid
    $verdict.evidence = if ($LASTEXITCODE -eq 0) { 'PASS' } else { 'FAIL' }
    $verdict.evidenceDetail = ($v -join ' ')
} else { $verdict.evidence = 'MISSING (boot did not reach evidence write)' }

$diag = @{}
if (Test-Path "$out\OMNI-DIAG.TXT") {
    foreach ($l in Get-Content "$out\OMNI-DIAG.TXT") { if ($l -match '^(OMNI_[A-Z0-9_]+)=(.*)$') { $diag[$Matches[1]] = $Matches[2] } elseif ($l -match '^OMNI_[A-Z_]+$') { $diag[$l] = 'present' } }
}
$expect = [ordered]@{
    OMNI_HDA_GPU_HDMI_SKIPPED = { param($x) [int]$x -ge 1 }
    OMNI_HDA_SELECTION_PASS = { param($x) $x -eq '0' }
    OMNI_HDA_VENDOR_ID = { param($x) $x -eq '4130' }            # 0x1022
    OMNI_HDA_DEVICE_ID = { param($x) $x -eq '5603' }            # 0x15E3
    OMNI_HDA_CODEC_VENDOR_ID = { param($x) $x -eq '283902550' } # 0x10EC0256 Realtek ALC256
    OMNI_HDA_ANALOG_PIN_CANDIDATES = { param($x) [int]$x -ge 1 }
    OMNI_HDA_DMA_PROGRESS = { param($x) $x -eq '1' }
    OMNI_HDA_ROUTE_PROGRAMMED = { param($x) $x -eq '1' }
}
$gates = [ordered]@{}
foreach ($k in $expect.Keys) {
    $val = $diag[$k]
    $gates[$k] = if ($null -eq $val) { 'MISSING' } elseif (& $expect[$k] $val) { "PASS ($val)" } else { "FAIL ($val)" }
}
$gates.OMNI_HDA_PIN_NODE = $diag.OMNI_HDA_PIN_NODE
$gates.KEYBOARD = if ($diag.ContainsKey('OMNI_KEYBOARD_UNPROVEN')) { 'UNPROVEN' } elseif ($diag.Count) { 'see OMNI_KEYBOARD_*' } else { 'MISSING' }
$verdict.gates = $gates
$reader = [ordered]@{ installedSha256 = $binding.screenReaderSha256 }
if (Test-Path "$out\QEVARYNOX-PHYSICAL-PROOF.TXT") {
    foreach ($l in Get-Content "$out\QEVARYNOX-PHYSICAL-PROOF.TXT") { if ($l -match '^(STATUS|HDA_CONTROLLER_SELECTION|HDA_CODEC_VENDOR_DEVICE|HDA_CODEC_SELECTION|HDA_SELECTED_PIN_DEFAULT_CONFIG|HII_GRAPH_SPEECH_MODE)=(.*)$') { $reader[$Matches[1]] = $Matches[2] } }
} else { $reader.STATUS = if ($binding.screenReaderSha256) { 'MISSING (reader did not reach its proof write)' } else { 'NOT INSTALLED' } }
$reader.trace = (Get-Content "$out\OMNI-TRACE.TXT" -ErrorAction SilentlyContinue) -join ' '
$verdict.screenReader = $reader
$verdict.speakerHeardByHuman = 'NOT RECORDED: ask the person at the machine'
$verdict | ConvertTo-Json -Depth 4 | Tee-Object "$out\VERDICT.json"
