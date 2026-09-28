param(
  [Parameter(Mandatory=$true)][string]$OutDir
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$letters = [ordered]@{
  a='a'; b='bé'; c='cé'; d='dé'; e='e'; f='èf'; g='gé'; h='ache'; i='i'; j='ji';
  k='ka'; l='elle'; m='ème'; n='ène'; o='o'; p='pé'; q='ku'; r='ère'; s='esse';
  t='té'; u='u'; v='vé'; w='double vé'; x='ixe'; y='i grec'; z='zède'
}
$digits = [ordered]@{
  '0'='zéro'; '1'='un'; '2'='deux'; '3'='trois'; '4'='quatre'; '5'='cinq';
  '6'='six'; '7'='sept'; '8'='huit'; '9'='neuf'
}
$words = @(
  'advanced','action','asus','back','bios','boot','button','change','checked',
  'configuration','cpu','default','device','disabled','enabled','enter','escape',
  'exit','help','left','main','memory','network','nvme','option','password',
  'processor','recovery','restore','right','save','secure','security','settings',
  'setup','storage','system','tpm','up','down','usb','value'
)
$phrases = @(
  "Prêt. F un, aide.",
  "Flèches pour naviguer.",
  "Entrée active. Échap retour.",
  "Aucun changement.",
  "Modifications annulées.",
  "Coché.",
  "Non coché.",
  "Protégé."
)

$probe = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voices = @($probe.GetInstalledVoices() | Where-Object { $_.Enabled })
if (-not $voices) { throw "No enabled System.Speech voice is available under this account." }

$frVoices = @($voices | Where-Object { $_.VoiceInfo.Culture.Name -like 'fr-*' })
$enVoices = @($voices | Where-Object { $_.VoiceInfo.Culture.Name -like 'en-*' })

function Select-PreferredVoice([object[]]$Candidates, [string[]]$PreferredNames) {
  foreach ($preferred in $PreferredNames) {
    $match = $Candidates | Where-Object {
      $_.VoiceInfo.Name -like "*$preferred*"
    } | Select-Object -First 1
    if ($match) { return $match }
  }
  $female = $Candidates | Where-Object {
    $_.VoiceInfo.Gender -eq [System.Speech.Synthesis.VoiceGender]::Female
  } | Select-Object -First 1
  if ($female) { return $female }
  return ($Candidates | Select-Object -First 1)
}

# Prefer the clearest female Windows voices when present. Narrator natural
# voices such as Denise are not guaranteed to be exposed through System.Speech,
# so the list deliberately falls back to the best installed desktop/OneCore
# voice instead of failing the physical build.
$frNative = Select-PreferredVoice $frVoices @('Denise','Hortense','Julie')
$enNative = Select-PreferredVoice $enVoices @('Aria','Jenny','Zira','Hazel')
$fr = if ($frNative) { $frNative } else { $voices | Select-Object -First 1 }
$en = if ($enNative) { $enNative } else { $fr }
$voiceInventory = ($voices | ForEach-Object {
  "$($_.VoiceInfo.Name)|$($_.VoiceInfo.Culture.Name)"
}) -join ';'

$voiceEvidence = @(
  "SYSTEM_SPEECH_VOICE_COUNT=$($voices.Count)"
  "SYSTEM_SPEECH_FR=$($fr.VoiceInfo.Name)"
  "SYSTEM_SPEECH_FR_CULTURE=$($fr.VoiceInfo.Culture.Name)"
  "SYSTEM_SPEECH_FR_GENDER=$($fr.VoiceInfo.Gender)"
  "SYSTEM_SPEECH_FR_NATIVE=$([bool]$frNative)"
  "SYSTEM_SPEECH_EN=$($en.VoiceInfo.Name)"
  "SYSTEM_SPEECH_EN_CULTURE=$($en.VoiceInfo.Culture.Name)"
  "SYSTEM_SPEECH_EN_GENDER=$($en.VoiceInfo.Gender)"
  "SYSTEM_SPEECH_EN_NATIVE=$([bool]$enNative)"
  "SYSTEM_SPEECH_VOICES=$voiceInventory"
  "SYSTEM_SPEECH_OUTPUT_RATE=16000"
  "SYSTEM_SPEECH_OUTPUT_BITS=16"
  "SYSTEM_SPEECH_OUTPUT_CHANNELS=1"
  "SYSTEM_SPEECH_OUTPUT_ENCODING=PCM"
)
$voiceEvidence | Set-Content -Path (Join-Path $OutDir 'system-voice-evidence.txt') -Encoding utf8
$probe.Dispose()

function Write-VoiceWav([string]$FileName, [string]$Text, [string]$VoiceName, [int]$Rate) {
  $path = Join-Path $OutDir $FileName
  $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
  try {
    $synth.SelectVoice($VoiceName)
    $synth.Rate = [Math]::Max(-2, [Math]::Min(2, $Rate))
    $synth.Volume = 100

    # UEFI v12 quality contract: make SAPI emit exactly the format embedded by
    # the firmware bank. This avoids an extra host-side sample-rate conversion,
    # which was adding aliasing to consonants before G.711 mu-law encoding.
    $format = [System.Speech.AudioFormat.SpeechAudioFormatInfo]::new(
      16000,
      [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
      [System.Speech.AudioFormat.AudioChannel]::Mono
    )
    $synth.SetOutputToWaveFile($path, $format)
    $synth.Speak($Text)
  }
  finally {
    $synth.Dispose()
  }
  if (-not (Test-Path $path) -or (Get-Item $path).Length -lt 128) {
    throw "Speech asset generation failed: $FileName"
  }
}

foreach ($key in $letters.Keys) {
  Write-VoiceWav ("letter_{0}.wav" -f $key) ([string]$letters[$key]) $fr.VoiceInfo.Name 0
}
foreach ($key in $digits.Keys) {
  Write-VoiceWav ("digit_{0}.wav" -f $key) ([string]$digits[$key]) $fr.VoiceInfo.Name 0
}
foreach ($w in $words) {
  Write-VoiceWav ("word_{0}.wav" -f $w) ([string]$w) $en.VoiceInfo.Name 0
}
for ($i = 0; $i -lt $phrases.Count; $i++) {
  Write-VoiceWav ("phrase_{0}.wav" -f $i) ([string]$phrases[$i]) $fr.VoiceInfo.Name 0
}

# Dedicated intelligibility reference: this file is not embedded as a firmware
# token. The PsExec physical workflow plays it, then plays the exact
# mu-law/decode/48 kHz roundtrip so a human can distinguish TTS quality from
# firmware codec/HDA corruption.
Write-VoiceWav 'quality_reference.wav' "Lecteur d'écran prêt. Navigation vocale active. Flèches pour naviguer. Entrée active. Échap retour." $fr.VoiceInfo.Name 0

$generated = @(Get-ChildItem -Path $OutDir -Filter '*.wav')
if ($generated.Count -lt 70) {
  throw "Too few real-voice assets generated: $($generated.Count)"
}
"REAL_VOICE_ASSET_COUNT=$($generated.Count)" | Add-Content -Path (Join-Path $OutDir 'system-voice-evidence.txt') -Encoding utf8
Write-Host "SYSTEM_SPEECH_REAL_VOICE=PASS"
Write-Host "SYSTEM_SPEECH_ASSET_COUNT=$($generated.Count)"
Write-Host "SYSTEM_SPEECH_FR_VOICE=$($fr.VoiceInfo.Name)"
Write-Host "SYSTEM_SPEECH_EN_VOICE=$($en.VoiceInfo.Name)"
