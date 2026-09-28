param(
  [string]$ProofPath,
  [string]$ExpectedSourceBlob,
  [switch]$AudibleSpeakerConfirmed
)
$ErrorActionPreference='Stop'

if(-not $ProofPath){
  $candidates=@()
  foreach($drive in [IO.DriveInfo]::GetDrives()){
    if(-not $drive.IsReady){ continue }
    $p=Join-Path $drive.RootDirectory.FullName 'QEVARYNOX-PHYSICAL-PROOF.TXT'
    if(Test-Path $p){ $candidates += $p }
  }
  if($candidates.Count -ne 1){
    throw "Expected exactly one QEVARYNOX-PHYSICAL-PROOF.TXT on mounted media; found $($candidates.Count)"
  }
  $ProofPath=$candidates[0]
}

$ProofPath=(Resolve-Path $ProofPath).Path
$raw=Get-Content $ProofPath -Raw
if($raw.Length -gt 65536){ throw 'Physical proof is unexpectedly large' }
if($raw.Contains([char]0)){ throw 'Physical proof contains NUL bytes' }
$text=$raw -replace "`r",""

$header='QEVARYNOX-UEFI-PHYSICAL-BOOT-PROOF-V1'
if(([regex]::Matches($text,"(?m)^$([regex]::Escape($header))$")).Count -ne 1){
  throw 'Physical proof header is missing or duplicated'
}

function Get-ProofField([string]$Name,[string]$Pattern='[^\r\n]+'){
  $matches=[regex]::Matches($text,"(?m)^$([regex]::Escape($Name))=($Pattern)$")
  if($matches.Count -ne 1){
    throw "Physical proof field missing, duplicated, or invalid: $Name"
  }
  return $matches[0].Groups[1].Value
}

$sourceBlob=Get-ProofField 'UEFI_SOURCE_BLOB' '[0-9A-Fa-f]{40}'
if($sourceBlob -cnotmatch '^[0-9a-f]{40}$'){
  throw "Physical proof source blob is not canonical lowercase Git SHA-1: $sourceBlob"
}
if($ExpectedSourceBlob){
  if($ExpectedSourceBlob -cnotmatch '^[0-9a-f]{40}$'){
    throw "Invalid expected source blob: $ExpectedSourceBlob"
  }
  if($sourceBlob -cne $ExpectedSourceBlob){
    throw "Physical proof source blob $sourceBlob does not match expected $ExpectedSourceBlob"
  }
}

$expected=[ordered]@{
  STATUS='PASS'
  HII_PROMPT_SOURCE='PASS'
  HII_GRAPH_SPEECH_MODE='CLEAR_LETTERNAME_SPELLING_FR_V3'
  HDA_CONTROLLER_SELECTION='PREFERRED_AMD_1022_15E3'
  HDA_CODEC_VENDOR_DEVICE='0x10EC0256'
  HDA_CODEC_SELECTION='REALTEK_10EC_0256'
  HDA_GRAPH_SEARCH_LIVE='PASS'
  HDA_SELECTOR_APPLY_LIVE='PASS'
  HDA_ROUTE_POWER_D0='PASS'
  HDA_ROUTE_AMPLIFIERS='PASS'
  HDA_EAPD_POLICY='PASS'
  HDA_DAC_STREAM_READBACK='PASS'
  HDA_PIN_CONTROL_READBACK='PASS'
  HDA_OUTPUT_PATH_CONFIGURATION='PASS'
  HII_GRAPH_SPEECH_DMA='PASS'
  LPIB_PROGRESS='PASS'
  PHYSICAL_ASUS_M1603QA_HDA_RUNTIME='PASS'
  PHYSICAL_ASUS_M1603QA_CODEC='REALTEK_10EC_0256'
  PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN='PASS'
  HII_GRAPH_NAV_UP='PASS'
  HII_GRAPH_NAV_DOWN='PASS'
  HII_GRAPH_NAV_HOME='PASS'
  HII_GRAPH_NAV_END='PASS'
  HII_GRAPH_NAV_PAGE_UP='PASS'
  HII_GRAPH_NAV_PAGE_DOWN='PASS'
  HII_GRAPH_NAV_REPEAT='PASS'
  HII_GRAPH_NAV_REQUIRED_EVENTS='PASS'
  HII_GRAPH_NAV_EXIT='PASS'
  HII_GRAPH_SPEECH_DMA_REUSE='PASS'
  AUDIBLE_PHYSICAL_SPEAKER='REQUIRES_HUMAN_CONFIRMATION'
}
foreach($name in $expected.Keys){
  $actual=Get-ProofField $name
  if($actual -ne $expected[$name]){
    throw "Physical proof field $name=$actual; expected $($expected[$name])"
  }
}

$pin=Get-ProofField 'HDA_PIN_NID' '0x[0-9A-F]{2}'
$dac=Get-ProofField 'HDA_DAC_NID' '0x[0-9A-F]{2}'
$depth=Get-ProofField 'HDA_ROUTE_DEPTH' '0x[0-9A-F]{2}'
$selectorsRequired=Get-ProofField 'HDA_SELECTOR_WRITES_REQUIRED' '0x[0-9A-F]{2}'
$selectorsApplied=Get-ProofField 'HDA_SELECTOR_WRITES_APPLIED' '0x[0-9A-F]{2}'
$navEvents=Get-ProofField 'HII_GRAPH_NAV_SPEECH_EVENTS' '0x[0-9A-F]{2}'

$pinValue=[Convert]::ToInt32($pin.Substring(2),16)
$dacValue=[Convert]::ToInt32($dac.Substring(2),16)
$depthValue=[Convert]::ToInt32($depth.Substring(2),16)
$requiredValue=[Convert]::ToInt32($selectorsRequired.Substring(2),16)
$appliedValue=[Convert]::ToInt32($selectorsApplied.Substring(2),16)
$navEventCount=[Convert]::ToInt32($navEvents.Substring(2),16)

if($pinValue -eq 0 -or $dacValue -eq 0 -or $pinValue -eq $dacValue){
  throw "Invalid physical HDA route endpoints: pin=$pin dac=$dac"
}
if($depthValue -lt 1 -or $depthValue -gt 16){
  throw "Invalid physical HDA route depth: $depth"
}
if($requiredValue -ne $appliedValue){
  throw "Physical selector write/readback mismatch: required=$selectorsRequired applied=$selectorsApplied"
}
if($navEventCount -lt 7){
  throw "Physical HII navigation produced only $navEventCount speech events; expected at least 7"
}

[pscustomobject]@{
  Result='PASS'
  ProofPath=$ProofPath
  SourceBlob=$sourceBlob
  Controller='PCI 1022:15E3'
  Codec='Realtek 10EC:0256 verified by native HDA verb'
  InternalSpeakerPath='PASS'
  PinNid=$pin
  DacNid=$dac
  RouteDepth=$depthValue
  SelectorWritesRequired=$requiredValue
  SelectorWritesApplied=$appliedValue
  NativeUefiHdaExecution='PASS'
  SpeechMode='CLEAR_LETTERNAME_SPELLING_FR_V3'
  HiiNavigation='UP_DOWN_HOME_END_PAGEUP_PAGEDOWN_R_ESC_PASS'
  NavigationSpeechEvents=$navEventCount
  DmaReuse='PASS'
  AudiblePhysicalSpeaker=$(if($AudibleSpeakerConfirmed){'PASS'}else{'REQUIRES_HUMAN_CONFIRMATION'})
} | ConvertTo-Json -Depth 4

'PHYSICAL_UEFI_SOURCE_BINDING=PASS'
'PHYSICAL_UEFI_HDA_EXECUTION=PASS'
'PHYSICAL_UEFI_CLEAR_SPEECH_MODE=PASS'
'PHYSICAL_UEFI_INTERNAL_SPEAKER_PATH=PASS'
'PHYSICAL_UEFI_HII_NAVIGATION=PASS'
'PHYSICAL_UEFI_DMA_REUSE=PASS'
if($AudibleSpeakerConfirmed){
  'PHYSICAL_UEFI_SPEAKER_AUDIBLE=PASS'
  'PHYSICAL_UEFI_FINAL_CLOSURE=PASS'
} else {
  'PHYSICAL_UEFI_SPEAKER_AUDIBLE=REQUIRES_HUMAN_CONFIRMATION'
  'PHYSICAL_UEFI_FINAL_CLOSURE=PENDING_AUDIBLE_CONFIRMATION'
}
