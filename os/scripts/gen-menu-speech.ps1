#Requires -Version 7.0
<#
.SYNOPSIS
    Regenerate the kernel boot menu's speech clips.

.DESCRIPTION
    The accessible boot menu (kernel/x86_64/src/boot_menu.rs) speaks each item
    through the HDA codec by playing a pre-recorded PCM clip, embedded in the
    kernel with include_bytes!. This synthesizes them with the Windows Speech API
    to 24 kHz 16-bit mono raw PCM - the format kernel/x86_64/src/hda.rs plays for
    speech. The clip filenames match the wiring in boot_menu.rs. The .pcm files
    are committed, so CI and the build use them as-is and never re-synthesize.
#>
[CmdletBinding()]
param(
    [string]$Voice = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Speech

$outDir = Join-Path (Split-Path $PSScriptRoot -Parent) 'kernel/x86_64/src/speech'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# Filename -> spoken text. Must match the utterances the menu voices in
# boot_menu.rs (the same wording aw-screen-reader produces).
$phrases = [ordered]@{
    'menu_title' = 'Accessible Windows, boot menu'
    'item_continue' = 'Continue and idle, menu item, 1 of 3'
    'item_sysinfo' = 'System information, menu item, 2 of 3'
    'item_reboot' = 'Reboot, menu item, 3 of 3'
}

$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    24000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
Write-Host 'Installed voices:'
$synth.GetInstalledVoices() | ForEach-Object { '  ' + $_.VoiceInfo.Name } | Write-Host
if ($Voice) { $synth.SelectVoice($Voice) }
Write-Host "Using voice: $($synth.Voice.Name)`n"

foreach ($name in $phrases.Keys) {
    $wav = Join-Path $env:TEMP "aw_menu_$name.wav"
    $synth.SetOutputToWaveFile($wav, $fmt)
    $synth.Speak($phrases[$name])
    $synth.SetOutputToNull()

    # Extract the raw PCM 'data' chunk from the WAV container.
    $bytes = [System.IO.File]::ReadAllBytes($wav)
    $pos = 12  # skip 'RIFF' <size> 'WAVE'
    $pcm = $null
    while ($pos -lt $bytes.Length - 8) {
        $id = [System.Text.Encoding]::ASCII.GetString($bytes, $pos, 4)
        $size = [BitConverter]::ToInt32($bytes, $pos + 4)
        if ($id -eq 'data') {
            $pcm = New-Object byte[] $size
            [Array]::Copy($bytes, $pos + 8, $pcm, 0, $size)
            break
        }
        $pos += 8 + $size + ($size % 2)
    }
    if (-not $pcm) { throw "no data chunk in $wav" }

    $pcmPath = Join-Path $outDir "$name.pcm"
    [System.IO.File]::WriteAllBytes($pcmPath, $pcm)
    Remove-Item -LiteralPath $wav -Force
    Write-Host ("  {0}.pcm: {1} bytes ({2:N2}s)" -f $name, $pcm.Length, ($pcm.Length / 48000.0))
}

$synth.Dispose()
Write-Host "`nWrote clips to $outDir"
