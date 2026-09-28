# sapi_gen.ps1 - Genere un WAV via SAPI 5 (SpVoice + SpFileStream).
# Args: -VoiceName <substring> -Text <text> -OutFile <wav-path> [-Rate 0] [-SampleRate 22050]
param(
    [Parameter(Mandatory=$true)][string]$VoiceName,
    [Parameter(Mandatory=$true)][string]$Text,
    [Parameter(Mandatory=$true)][string]$OutFile,
    [int]$Rate = 0,
    [int]$SampleRate = 22050
)

$ErrorActionPreference = 'Stop'
$outDir = Split-Path -Parent $OutFile
if ($outDir -and -not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
}

# Format audio cible : 16-bit MONO (constantes SAPI SpeechAudioFormatType)
# SpeechAudioFormatType: explicit 16-bit PCM mono, not 8-bit PCM.
$formatMap = @{ 8000 = 6; 11025 = 10; 16000 = 18; 22050 = 22; 24000 = 26; 44100 = 34 }
if (-not $formatMap.ContainsKey($SampleRate)) { throw "Unsupported sample rate" }
$fmtType = $formatMap[$SampleRate]

# NB : la variable locale est $sp (pas $voice) pour eviter la collision case-insensitive
#     avec le parametre $VoiceName et la propriete COM .Voice.
$sp = New-Object -ComObject SAPI.SpVoice
$voicesCat = $sp.GetVoices()
$chosen = $null
for ($i = 0; $i -lt $voicesCat.Count; $i++) {
    $tok = $voicesCat.Item($i)
    $desc = $tok.GetDescription()
    if ($desc -like "*$VoiceName*") {
        $chosen = $tok
        break
    }
}
if ($null -eq $chosen) {
    Write-Error "Voix '$VoiceName' introuvable"
    exit 2
}
$sp.Voice = $chosen
$sp.Rate = $Rate
$sp.Volume = 100

$fmt = New-Object -ComObject SAPI.SpAudioFormat
$fmt.Type = $fmtType
$stream = New-Object -ComObject SAPI.SpFileStream
$stream.Format = $fmt
$stream.Open($OutFile, 3, $false)   # SSFMCreateForWrite = 3
$sp.AudioOutputStream = $stream
try { $sp.Speak($Text, 0) }   # flags=0 (synchrone, sans purge)
finally {
    $stream.Close()
    [System.Runtime.Interopservices.Marshal]::ReleaseComObject($stream) | Out-Null
    [System.Runtime.Interopservices.Marshal]::ReleaseComObject($fmt) | Out-Null
    [System.Runtime.Interopservices.Marshal]::ReleaseComObject($sp) | Out-Null
    [System.Runtime.Interopservices.Marshal]::ReleaseComObject($voicesCat) | Out-Null
    [System.Runtime.Interopservices.Marshal]::ReleaseComObject($chosen) | Out-Null
    [GC]::Collect()
}
Write-Output "OK"
