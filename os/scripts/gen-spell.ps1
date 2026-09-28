#Requires -Version 7.0
<#
.SYNOPSIS
    Regenerate the spelling alphabet the accessible firmware setup uses to read
    dynamic text (boot-device names, machine-state values) character by character.

.DESCRIPTION
    The setup's fixed lines carry pre-recorded clips, but dynamic lines - the
    enumerated Boot#### device names, and the Main/Advanced/Security values - are
    composed at runtime and cannot be pre-recorded whole. A screen reader's answer is
    "read by character": this synthesizes one clip per letter (a-z), digit (0-9) and
    space, so boot/uefi/src/setup.rs can spell any dynamic line aloud on demand (the
    "S" key) through the same HDA codec and the same voice as every other clip. The
    .pcm files are committed, so CI and the build use them as-is.
#>
[CmdletBinding()]
param(
    [string]$Voice = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Speech

$outDir = Join-Path (Split-Path $PSScriptRoot -Parent) 'boot/uefi/src/speech'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# Clip name -> spoken text. The setup maps a character to the clip: letters to the
# letter's name, digits to the digit's name, and a space to the word "space".
$phrases = [ordered]@{}
foreach ($letter in [char[]]([int][char]'a'..[int][char]'z')) {
    $phrases["spell_$letter"] = [string]$letter
}
foreach ($digit in 0..9) {
    $phrases["spell_$digit"] = [string]$digit
}
$phrases['spell_space'] = 'space'

$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    24000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)

# Punctuation and symbols that appear in real firmware values (1280x800, USB 3.0,
# dates, percentages, boot paths). Without these, a value's separators are lost when
# spelled, so "3.0" would read "three zero". Their spoken NAME differs by language, so
# each is generated twice: an English clip (spell_<name>.pcm) spoken with an English
# voice, and a French clip (fr_spell_<name>.pcm) spoken with a French voice. The setup
# picks the set for the active language. Letters and digits stay language-neutral.
$punct = [ordered]@{
    'dot'        = @{ char = '.'; en = 'dot';                 fr = 'point' }
    'dash'       = @{ char = '-'; en = 'dash';                fr = 'tiret' }
    'colon'      = @{ char = ':'; en = 'colon';               fr = 'deux points' }
    'slash'      = @{ char = '/'; en = 'slash';               fr = 'barre oblique' }
    'backslash'  = @{ char = '\'; en = 'backslash';           fr = 'barre oblique inverse' }
    'percent'    = @{ char = '%'; en = 'percent';             fr = 'pour cent' }
    'comma'      = @{ char = ','; en = 'comma';               fr = 'virgule' }
    'underscore' = @{ char = '_'; en = 'underscore';          fr = 'souligne' }
    'lparen'     = @{ char = '('; en = 'open parenthesis';    fr = 'parenthese ouvrante' }
    'rparen'     = @{ char = ')'; en = 'close parenthesis';   fr = 'parenthese fermante' }
    'plus'       = @{ char = '+'; en = 'plus';                fr = 'plus' }
    'equals'     = @{ char = '='; en = 'equals';              fr = 'egal' }
    'at'         = @{ char = '@'; en = 'at';                  fr = 'arobase' }
}

# Pick the raw PCM 'data' chunk out of the WAV container the synthesizer writes.
function Get-PcmData([string]$Wav) {
    $bytes = [System.IO.File]::ReadAllBytes($Wav)
    $pos = 12  # skip 'RIFF' <size> 'WAVE'
    while ($pos -lt $bytes.Length - 8) {
        $id = [System.Text.Encoding]::ASCII.GetString($bytes, $pos, 4)
        $size = [BitConverter]::ToInt32($bytes, $pos + 4)
        if ($id -eq 'data') {
            $pcm = New-Object byte[] $size
            [Array]::Copy($bytes, $pos + 8, $pcm, 0, $size)
            return $pcm
        }
        $pos += 8 + $size + ($size % 2)
    }
    throw "no data chunk in $Wav"
}

# Write one clip: synthesize $text on $synth into $name.pcm.
function Write-Clip($synth, [string]$name, [string]$text) {
    $wav = Join-Path $env:TEMP "aw_$name.wav"
    $synth.SetOutputToWaveFile($wav, $fmt)
    $synth.Speak($text)
    $synth.SetOutputToNull()
    $pcm = Get-PcmData $wav
    [System.IO.File]::WriteAllBytes((Join-Path $outDir "$name.pcm"), $pcm)
    Remove-Item -LiteralPath $wav -Force
    Write-Host ("  {0}.pcm: {1} bytes" -f $name, $pcm.Length)
}

# Resolve one installed voice whose culture starts with $prefix (e.g. 'en', 'fr').
function Resolve-Voice($synth, [string]$prefix) {
    $v = $synth.GetInstalledVoices() |
        Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.Name -like "$prefix*" } |
        Select-Object -First 1
    if (-not $v) { throw "No installed $prefix voice; cannot generate $prefix clips." }
    return $v.VoiceInfo.Name
}

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer

# 1) Language-neutral alphabet (letters, digits, space), as before.
if ($Voice) { $synth.SelectVoice($Voice) }
Write-Host "Alphabet voice: $($synth.Voice.Name)`n"
foreach ($name in $phrases.Keys) { Write-Clip $synth $name $phrases[$name] }

# 2) English punctuation, spoken with an English voice.
$enVoice = Resolve-Voice $synth 'en'
$synth.SelectVoice($enVoice)
Write-Host "`nEnglish punctuation voice: $enVoice"
foreach ($name in $punct.Keys) { Write-Clip $synth "spell_$name" $punct[$name].en }

# 3) French punctuation, spoken with a French voice.
$frVoice = Resolve-Voice $synth 'fr'
$synth.SelectVoice($frVoice)
Write-Host "`nFrench punctuation voice: $frVoice"
foreach ($name in $punct.Keys) { Write-Clip $synth "fr_spell_$name" $punct[$name].fr }

# 4) NATO phonetic alphabet - unambiguous letter names (Alpha, Bravo, Charlie...) for a
# user who turns on phonetic spelling, so b/d/p and m/n are never confused. One set,
# spoken with the English voice; the NATO words are used the same internationally.
$nato = [ordered]@{
    a = 'Alpha';   b = 'Bravo';    c = 'Charlie'; d = 'Delta';   e = 'Echo'
    f = 'Foxtrot'; g = 'Golf';     h = 'Hotel';   i = 'India';   j = 'Juliett'
    k = 'Kilo';    l = 'Lima';     m = 'Mike';    n = 'November'; o = 'Oscar'
    p = 'Papa';    q = 'Quebec';   r = 'Romeo';   s = 'Sierra';  t = 'Tango'
    u = 'Uniform'; v = 'Victor';   w = 'Whiskey'; x = 'X-ray';   y = 'Yankee'
    z = 'Zulu'
}
$synth.SelectVoice($enVoice)
Write-Host "`nNATO phonetic voice: $enVoice"
foreach ($letter in $nato.Keys) { Write-Clip $synth "spell_nato_$letter" $nato[$letter] }

$synth.Dispose()
Write-Host "`nWrote spelling clips to $outDir"
