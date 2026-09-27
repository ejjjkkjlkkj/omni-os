# After the physical OMNI boot: copy the evidence off the key, verify it fail-closed
# against the run binding written by omni_deploy_physical.ps1, and compare the HDA
# gates with docs/HDA_ANALOG_ROUTE.md. Read-only on the key.
#
#   C:\Tools\PsExec\PsExec64.exe -accepteula -s -h -w <repo> pwsh.exe -NoProfile -ExecutionPolicy Bypass `n#       -File tools\omni_collect_physical.ps1 [-Label OMNI] [-PlatformUuid bb58f448-...]
param(
    [string]$Label = 'OMNI',
    [string]$OutRoot = 'C:\OMNI-PHYSICAL-EVIDENCE',
    [string]$PlatformUuid = 'bb58f448-c083-e24c-acfb-00153fe8bb5a',
    [switch]$AudibleSpeakerConfirmed
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
    OMNI_HDA_ROUTE_RESOLVED = { param($x) $x -eq '1' }
    OMNI_HDA_DMA_PROGRESS = { param($x) $x -eq '1' }
    OMNI_HDA_ROUTE_PROGRAMMED = { param($x) $x -eq '1' }
}
$gates = [ordered]@{}
foreach ($k in $expect.Keys) {
    $val = $diag[$k]
    $gates[$k] = if ($null -eq $val) { 'MISSING' } elseif (& $expect[$k] $val) { "PASS ($val)" } else { "FAIL ($val)" }
}
$gates.OMNI_HDA_PIN_NODE = $diag.OMNI_HDA_PIN_NODE
$gates.OMNI_HDA_CONVERTER_NODE = $diag.OMNI_HDA_CONVERTER_NODE
$gates.OMNI_HDA_ROUTE_RESOLVED_DEPTH = $diag.OMNI_HDA_ROUTE_RESOLVED_DEPTH
$gates.OMNI_HDA_ROUTE_INTERMEDIATE_NODE = $diag.OMNI_HDA_ROUTE_INTERMEDIATE_NODE
$gates.OMNI_HDA_ROUTE_INTERMEDIATE_CONNECTION_INDEX = $diag.OMNI_HDA_ROUTE_INTERMEDIATE_CONNECTION_INDEX
$gates.OMNI_HDA_ROUTE_INTERMEDIATE_AMP_PROGRAMMED = $diag.OMNI_HDA_ROUTE_INTERMEDIATE_AMP_PROGRAMMED
$gates.KEYBOARD = if ($diag.ContainsKey('OMNI_KEYBOARD_PASS')) { 'PASS' } elseif ($diag.ContainsKey('OMNI_KEYBOARD_UNPROVEN')) { 'UNPROVEN' } elseif ($diag.Count) { 'MISSING' } else { 'MISSING' }
$gates.NAVIGATION_INPUT = if ($diag.ContainsKey('OMNI_NAVIGATION_INPUT_PASS')) { 'PASS' } elseif ($diag.ContainsKey('OMNI_NAVIGATION_INPUT_UNPROVEN')) { 'UNPROVEN' } else { 'MISSING' }
$verdict.gates = $gates
$reader = [ordered]@{ installedSha256 = $binding.screenReaderSha256; actualSha256 = $null }
$readerPath = "$out\EFI\OMNI\SCREENREADER.EFI"
if (Test-Path $readerPath) {
    $reader.actualSha256 = (Get-FileHash $readerPath -Algorithm SHA256).Hash.ToLower()
}
if (Test-Path "$out\QEVARYNOX-PHYSICAL-PROOF.TXT") {
    foreach ($l in Get-Content "$out\QEVARYNOX-PHYSICAL-PROOF.TXT") {
        if ($l -match '^([A-Z0-9_]+)=(.*)$') { $reader[$Matches[1]] = $Matches[2] }
    }
} else { $reader.STATUS = if ($binding.screenReaderSha256) { 'MISSING (reader did not reach its proof write)' } else { 'NOT INSTALLED' } }
$reader.trace = (Get-Content "$out\OMNI-TRACE.TXT" -ErrorAction SilentlyContinue) -join ' '
$readerProofPresent = Test-Path "$out\QEVARYNOX-PHYSICAL-PROOF.TXT"
$readerRequired = [ordered]@{
    STATUS = 'PASS'
    HII_GRAPH_SPEECH_MODE = 'CLEAR_LETTERNAME_SPELLING_FR_V3'
    HDA_CONTROLLER_SELECTION = 'PREFERRED_AMD_1022_15E3'
    HDA_CODEC_VENDOR_DEVICE = '0x10ec0256'
    HDA_CODEC_SELECTION = 'REALTEK_10EC_0256'
    HDA_GRAPH_SEARCH_LIVE = 'PASS'
    HDA_SELECTOR_APPLY_LIVE = 'PASS'
    HDA_ROUTE_POWER_D0 = 'PASS'
    HDA_ROUTE_AMPLIFIERS = 'PASS'
    HDA_EAPD_POLICY = 'PASS'
    HDA_DAC_STREAM_READBACK = 'PASS'
    HDA_PIN_CONTROL_READBACK = 'PASS'
    HDA_OUTPUT_PATH_CONFIGURATION = 'PASS'
    HII_GRAPH_SPEECH_DMA = 'PASS'
    LPIB_PROGRESS = 'PASS'
    PHYSICAL_ASUS_M1603QA_HDA_RUNTIME = 'PASS'
    PHYSICAL_ASUS_M1603QA_CODEC = 'REALTEK_10EC_0256'
    PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN = 'PASS'
    HII_GRAPH_NAV_REQUIRED_EVENTS = 'PASS'
    HII_GRAPH_NAV_REALTIME = 'PASS'
    HII_GRAPH_SPEECH_DMA_REUSE = 'PASS'
}
$verdict.screenReader = $reader
$verdict.speakerHeardByHuman = if ($AudibleSpeakerConfirmed) { 'CONFIRMED' } else { 'NOT RECORDED' }

$blockers = [Collections.Generic.List[string]]::new()
if ($verdict.evidence -ne 'PASS') { $blockers.Add("attested UEFI evidence is $($verdict.evidence)") }
foreach ($k in $expect.Keys) {
    if ($gates[$k] -notlike 'PASS*') { $blockers.Add("$k is $($gates[$k])") }
}
if ($gates.KEYBOARD -ne 'PASS') { $blockers.Add("physical keyboard DOWN+ENTER is $($gates.KEYBOARD)") }
if ($gates.NAVIGATION_INPUT -ne 'PASS') { $blockers.Add("physical navigation input is $($gates.NAVIGATION_INPUT)") }
if (-not $binding.screenReaderSha256) {
    $blockers.Add('screen reader was not bound into this physical run')
} elseif (-not $reader.actualSha256) {
    $blockers.Add('bound screen-reader binary is missing from collected media')
} elseif (-not [string]::Equals([string]$reader.actualSha256, [string]$binding.screenReaderSha256, [StringComparison]::OrdinalIgnoreCase)) {
    $blockers.Add("screen-reader SHA-256 mismatch: actual=$($reader.actualSha256) expected=$($binding.screenReaderSha256)")
} elseif (-not $readerProofPresent) {
    $blockers.Add('QEVARYNOX-PHYSICAL-PROOF.TXT is missing')
} else {
    foreach ($k in $readerRequired.Keys) {
        $actual = if ($reader.Contains($k)) { [string]$reader[$k] } else { $null }
        $expectedValue = [string]$readerRequired[$k]
        if ($null -eq $actual) {
            $blockers.Add("screen-reader proof $k is MISSING")
        } elseif (-not [string]::Equals($actual, $expectedValue, [StringComparison]::OrdinalIgnoreCase)) {
            $blockers.Add("screen-reader proof $k is '$actual' (expected '$expectedValue')")
        }
    }
}
if (-not $AudibleSpeakerConfirmed) { $blockers.Add('internal-speaker tone requires human confirmation') }

$verdict.releaseReady = ($blockers.Count -eq 0)
$verdict.releaseBlockers = @($blockers)
$verdict | ConvertTo-Json -Depth 5 | Tee-Object "$out\VERDICT.json"

if ($verdict.releaseReady) {
    Write-Host '[PASS] PHYSICAL RELEASE GATES COMPLETE'
    exit 0
}
Write-Host '[BLOCKED] physical release gates are incomplete:'
$blockers | ForEach-Object { Write-Host " - $_" }
exit 2
