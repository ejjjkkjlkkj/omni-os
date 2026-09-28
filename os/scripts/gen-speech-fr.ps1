#Requires -Version 7.0
<#
.SYNOPSIS
    Regenerate the French speech clips for the accessible firmware Setup Utility.

.DESCRIPTION
    The setup can be operated in French or English (a Language item switches, like the
    "System Language" option a real ASUS/AMI BIOS offers). English clips are produced by
    gen-speech.ps1; this produces the French set (prefix "fr_"), spoken by the installed
    French voice so they sound natural. Filenames match the clip wiring in
    boot/uefi/src/setup.rs. The .pcm files are committed, so CI and the build use them
    as-is and never re-synthesize.
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

# Filename -> spoken French text. Must match the French labels in setup.rs.
$phrases = [ordered]@{
    'fr_intro'            = 'Accessible Windows, utilitaire de configuration'
    'fr_instructions'     = 'Utilisez les flèches pour naviguer, Entrée pour sélectionner, Échap pour revenir, Espace pour répéter, A pour tout lire, S pour épeler, H pour l''aide'
    'fr_tab_main'         = 'Principal, onglet'
    'fr_tab_advanced'     = 'Avancé, onglet'
    'fr_tab_boot'         = 'Démarrage, onglet'
    'fr_tab_security'     = 'Sécurité, onglet'
    'fr_tab_saveexit'     = 'Enregistrer et quitter, onglet'
    'fr_act_boot_normally' = 'Démarrer normalement'
    'fr_act_enter_setup'  = 'Entrer dans la configuration du firmware'
    'fr_act_reset'        = 'Redémarrer le système'
    'fr_act_shutdown'     = 'Éteindre le système'
    'fr_sub_cpu'          = 'Configuration du processeur, sous-menu'
    'fr_sub_boot_prio'    = 'Priorités de démarrage, sous-menu'
    'fr_sub_secure_boot'  = 'Secure Boot, sous-menu'
    'fr_act_boot_now'     = 'Démarrer ce périphérique maintenant'
    'fr_act_make_default' = 'Définir comme périphérique de démarrage par défaut'
    'fr_act_move_up'      = 'Monter dans l''ordre de démarrage'
    'fr_act_move_down'    = 'Descendre dans l''ordre de démarrage'
    'fr_act_back'         = 'Revenir'
    'fr_boot_device'      = 'Périphérique de démarrage'
    'fr_confirm_prompt'   = 'Appuyez à nouveau sur Entrée pour confirmer, ou Échap pour annuler'
    'fr_confirm_cancel'   = 'Annulé'
    'fr_confirm_done'     = 'C''est fait'
    'fr_lang'             = 'Langue, Français'
}

$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    24000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
Write-Host 'Installed voices:'
$synth.GetInstalledVoices() | ForEach-Object { '  ' + $_.VoiceInfo.Name + ' (' + $_.VoiceInfo.Culture.Name + ')' } | Write-Host
# Prefer an installed French voice so the French clips sound native.
if (-not $Voice) {
    $fr = $synth.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like 'fr*' -and $_.Enabled } | Select-Object -First 1
    if ($fr) { $Voice = $fr.VoiceInfo.Name }
}
if ($Voice) { $synth.SelectVoice($Voice) }
Write-Host "Using voice: $($synth.Voice.Name)`n"

foreach ($name in $phrases.Keys) {
    $wav = Join-Path $env:TEMP "aw_$name.wav"
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
    Write-Host ("  {0}.pcm: {1} bytes" -f $name, $pcm.Length)
}

$synth.Dispose()
Write-Host "`nWrote French clips to $outDir"
