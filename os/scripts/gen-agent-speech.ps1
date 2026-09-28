#Requires -Version 7.0
<#
.SYNOPSIS
    Generate the spoken responses of the accessible UEFI command agent, in English
    and French.

.DESCRIPTION
    The agent lets a blind user TYPE a plain instruction ("boot usb", "secure boot",
    "restart") instead of navigating the setup tree, and speaks back what it
    understood and did. Its fixed replies are pre-recorded here as 24 kHz mono PCM,
    one clip per reply per language (agent_<name>.pcm in English, fr_agent_<name>.pcm
    in French), matching the rest of the firmware speech bank. Dynamic details
    (the exact Secure Boot / virtualization state, device names) are still read from
    live machine state and spelled through the alphabet bank; these clips are the
    fixed scaffolding the agent speaks around them.
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech

$outDir = Join-Path (Split-Path $PSScriptRoot -Parent) 'boot/uefi/src/speech'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# Clip name -> English / French reply.
$replies = [ordered]@{
    'prompt'          = @{ en = 'Command. Type an instruction, Enter to run, Escape to cancel.';        fr = "Commande. Tapez une instruction, Entree pour lancer, Echap pour annuler." }
    'help'            = @{ en = 'Commands: list, boot a device by name or number, set default, move up, move down, list settings, enable or disable a setting by name, secure boot, virtualization, time, memory, processor, firmware version, set timeout, firmware setup, system information, restart, shut down.'; fr = "Commandes : liste, demarrer un peripherique par nom ou numero, definir par defaut, monter, descendre, lister les reglages, activer ou desactiver un reglage par nom, secure boot, virtualisation, heure, memoire, processeur, version du firmware, definir le delai, configuration du firmware, informations systeme, redemarrer, eteindre." }
    'unknown'         = @{ en = 'Command not recognized. Type help for the list.';                       fr = "Commande non reconnue. Tapez aide pour la liste." }
    'firmware_only'   = @{ en = 'This is a firmware setting I cannot change from here. Opening firmware setup so you can change it.'; fr = "Ce reglage appartient au firmware, je ne peux pas le changer d'ici. Ouverture de la configuration du firmware pour que vous puissiez le changer." }
    'opening_setup'   = @{ en = 'Opening firmware setup and restarting.';                                fr = "Ouverture de la configuration du firmware et redemarrage." }
    'setup_denied'    = @{ en = 'This firmware does not allow opening its setup from here.';             fr = "Ce firmware n'autorise pas l'ouverture de sa configuration d'ici." }
    'restarting'      = @{ en = 'Restarting the system.';                                                fr = "Redemarrage du systeme." }
    'shutting_down'   = @{ en = 'Shutting down the system.';                                             fr = "Arret du systeme." }
    'secure_boot_is'  = @{ en = 'Secure Boot is';                                                        fr = "Secure Boot est" }
    'value_is'        = @{ en = 'Current value:';                                                        fr = "Valeur actuelle :" }
    'booting'         = @{ en = 'Booting the selected device now.';                                      fr = "Demarrage du peripherique selectionne." }
    'set_default'     = @{ en = 'Set as the default boot device.';                                       fr = "Defini comme peripherique de demarrage par defaut." }
    'no_match'        = @{ en = 'No matching boot device. Type list to hear them.';                      fr = "Aucun peripherique de demarrage correspondant. Tapez liste pour les entendre." }
    'boot_list'       = @{ en = 'Boot devices:';                                                         fr = "Peripheriques de demarrage :" }
    'done'            = @{ en = 'Done.';                                                                 fr = "C'est fait." }
    'failed'          = @{ en = 'That could not be done.';                                               fr = "Cela n'a pas pu etre fait." }
    'time_is'         = @{ en = 'The time is';                                                           fr = "L'heure est" }
    'memory_is'       = @{ en = 'Installed memory:';                                                     fr = "Memoire installee :" }
    'processor_is'    = @{ en = 'Processor:';                                                            fr = "Processeur :" }
    'firmware_is'     = @{ en = 'Firmware:';                                                             fr = "Micrologiciel :" }
    'timeout_set'     = @{ en = 'Boot timeout set to';                                                   fr = "Delai de demarrage regle sur" }
}

$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    24000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)

function Get-PcmData([string]$Wav) {
    $bytes = [System.IO.File]::ReadAllBytes($Wav)
    $pos = 12
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

function Resolve-Voice($synth, [string]$prefix) {
    $v = $synth.GetInstalledVoices() |
        Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.Name -like "$prefix*" } |
        Select-Object -First 1
    if (-not $v) { throw "No installed $prefix voice; cannot generate $prefix clips." }
    return $v.VoiceInfo.Name
}

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer

$enVoice = Resolve-Voice $synth 'en'
$synth.SelectVoice($enVoice)
Write-Host "English agent voice: $enVoice"
foreach ($name in $replies.Keys) { Write-Clip $synth "agent_$name" $replies[$name].en }

$frVoice = Resolve-Voice $synth 'fr'
$synth.SelectVoice($frVoice)
Write-Host "`nFrench agent voice: $frVoice"
foreach ($name in $replies.Keys) { Write-Clip $synth "fr_agent_$name" $replies[$name].fr }

$synth.Dispose()
Write-Host "`nWrote agent speech clips to $outDir"
