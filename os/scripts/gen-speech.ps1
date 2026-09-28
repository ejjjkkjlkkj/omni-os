#Requires -Version 7.0
<#
.SYNOPSIS
    Regenerate the boot screen's speech clips for the firmware-stage screen reader.

.DESCRIPTION
    The UEFI screen reader speaks its fixed lines through the HDA codec by playing
    pre-recorded PCM clips (boot/uefi/src/speech/*.pcm), embedded in the .efi with
    include_bytes!. This script synthesizes them with the Windows Speech API to
    24 kHz 16-bit mono raw PCM (the format boot/uefi/src/hda.rs plays).

    Run it to change the wording or the voice. Pick a voice whose language matches
    the phrases with -Voice (see the installed voices it prints); the default is
    the system voice, which may not be English. The .pcm files are committed, so
    CI and the build use them as-is and never re-synthesize.
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

# Filename -> spoken text. Must match the utterances in boot/uefi/src/screen_reader.rs
# and boot/uefi/src/setup.rs, and the clip wiring there. The four boot-screen lines are
# read first (spoken by `run`). The accessible firmware Setup Utility (setup.rs) then
# speaks its FIXED scaffolding aloud - the setup intro, the five tab names, and the
# fixed action items - from these clips, so a blind user hears the whole navigable
# structure, not just the intro. Only the DYNAMIC lines (Main/Advanced/Security values
# and the enumerated Boot#### device names, composed at runtime from real machine
# state) carry no clip; they are spoken on the console with an audible move cue, and
# runtime speech synthesis for them is future work. Clip text omits the "N of M"
# position suffix on purpose: the item count varies with how many boot entries the
# firmware exposes, so the audio speaks the stable label while the console and the
# proof markers carry the full positional utterance.
$phrases = [ordered]@{
    'welcome'           = 'Accessible Windows, window'
    'active'            = 'Screen reader active at firmware stage'
    'starting'          = 'Starting Accessible Windows'
    'loading'           = 'Loading the operating system'
    'menu_intro'        = 'Accessible Windows, setup utility'
    'tab_main'          = 'Main, tab'
    'tab_advanced'      = 'Advanced, tab'
    'tab_boot'          = 'Boot, tab'
    'tab_security'      = 'Security, tab'
    'tab_saveexit'      = 'Save and Exit, tab'
    'act_boot_normally' = 'Boot normally'
    'act_enter_setup'   = 'Enter firmware setup'
    'act_reset'         = 'Reset the system'
    'act_shutdown'      = 'Shut down the system'
    # Submenu titles and the actions inside them, so descending into a submenu and
    # every choice there is spoken too. Boot-device names stay dynamic (a generic
    # "Boot device" clip marks each, with the exact name on the console).
    'sub_cpu'           = 'CPU Configuration, submenu'
    'sub_boot_prio'     = 'Boot Option Priorities, submenu'
    'sub_secure_boot'   = 'Secure Boot, submenu'
    'act_boot_now'      = 'Boot this device now'
    'act_make_default'  = 'Make this the default boot device'
    'act_move_up'       = 'Move up in boot order'
    'act_move_down'     = 'Move down in boot order'
    'act_back'          = 'Go back'
    'boot_device'       = 'Boot device'
    # Ease of use: a clear result for every action, and a confirmation step before an
    # irreversible one, so a single accidental key press never reboots the machine.
    'confirm_prompt'    = 'Press Enter again to confirm, or Escape to cancel'
    'confirm_cancel'    = 'Cancelled'
    'confirm_done'      = 'Done'
    # The language selector, shown/spoken in English when the setup is in English (its
    # French counterpart, fr_lang, is in gen-speech-fr.ps1). Real BIOSes offer the same.
    'act_language'      = 'Language, English'
    # Spoken once on entry, so a blind user hears the interaction model up front.
    'instructions'      = 'Use the arrow keys to move, Enter to select, Escape to go back, Space to repeat, A to read all, S to spell, H for help'
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
    $wav = Join-Path $env:TEMP "aw_speech_$name.wav"
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
