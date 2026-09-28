#Requires -Version 7.0
<#
.SYNOPSIS
    Generate the premium-voice word bank: real-voice clips of the number words and the common
    firmware words, packed per language into one blob plus a Rust index.

.DESCRIPTION
    The fixed setup scaffolding is spoken by pre-recorded clips of a real installed voice, so it
    sounds native. The dynamic text (device names, values, numbers) was only formant-synthesized
    or spelled. This extends the real voice to the *words* that dynamic text is built from: every
    number word and a curated set of common firmware words, in French and English, each spoken by
    the installed premium voice, silence-trimmed, and packed into `word_bank_<lang>.bin` with a
    generated Rust index (`word_bank_gen.rs`). At runtime `word_bank.rs` looks a word up and plays
    its real-voice clip; anything not in the bank falls back to the runtime formant synthesizer.

    The clips are 24 kHz mono 16-bit PCM - the format the audio backends stream - so a word plays
    through the same DMA path as a fixed clip. The blobs and the generated index are committed, so
    the build never re-synthesizes.
#>
[CmdletBinding()]
param(
    [string]$FrenchVoice = '',
    [string]$EnglishVoice = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech

$root = Split-Path $PSScriptRoot -Parent
$speechDir = Join-Path $root 'boot/uefi/src/speech'
New-Item -ItemType Directory -Force -Path $speechDir | Out-Null

# ---- Word lists ---------------------------------------------------------------------------

# French: the atoms number-to-words emits, then common firmware label/value words.
$frWords = @(
    'zéro','un','deux','trois','quatre','cinq','six','sept','huit','neuf',
    'dix','onze','douze','treize','quatorze','quinze','seize','vingt','trente','quarante',
    'cinquante','soixante','cent','mille','million','et','virgule','point','pour',
    'système','numéro','série','fournisseur','firmware','version','heure','mémoire','installée',
    'méga','octets','résolution','affichage','par','processeur','technologie','virtualisation',
    'activé','désactivé','pris','en','charge','mode','configuration','utilisateur','démarrage',
    'actuel','délai','menu','secondes','clé','de','plateforme','échange','base','autorisée',
    'interdite','certificats','empreintes','provisionné','présent','banques','actives','aucune',
    'interface','module','démarrer','normalement','priorités','périphérique','sécurité','avancé',
    'principal','enregistrer','quitter','revenir','définir','défaut','monter','descendre','ordre',
    'redémarrer','éteindre','entrer','dans','la','du','le','les',
    'réglable','entrée','changer','ouvrir','appuyez','inconnu','processus'
)

# English: number atoms, then common firmware words and the words in real device names.
$enWords = @(
    'zero','one','two','three','four','five','six','seven','eight','nine',
    'ten','eleven','twelve','thirteen','fourteen','fifteen','sixteen','seventeen','eighteen','nineteen',
    'twenty','thirty','forty','fifty','sixty','seventy','eighty','ninety','hundred','thousand',
    'million','point','percent','by',
    'system','serial','number','firmware','vendor','version','time','memory','installed','megabytes',
    'display','resolution','processor','virtualization','technology','enabled','disabled','supported',
    'secure','boot','setup','mode','current','timeout','seconds','platform','key','database',
    'certificates','hashes','provisioned','present','banks','active','none','interface','module',
    'device','manager','disk','hard','drive','internal','shell','standard','windows','normally',
    'priorities','make','default','move','up','down','reset','shut','enter','the','of','not',
    'adjustable','open','change','press','unknown'
)

# ---- Synthesis ---------------------------------------------------------------------------

$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    24000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
Write-Host 'Installed voices:'
$synth.GetInstalledVoices() | ForEach-Object { '  ' + $_.VoiceInfo.Name + ' (' + $_.VoiceInfo.Culture.Name + ')' } | Write-Host

function Resolve-Voice([string]$requested, [string]$culturePrefix) {
    if ($requested) { return $requested }
    $v = $synth.GetInstalledVoices() |
        Where-Object { $_.VoiceInfo.Culture.Name -like "$culturePrefix*" -and $_.Enabled } |
        Select-Object -First 1
    if ($v) { return $v.VoiceInfo.Name }
    throw "No enabled $culturePrefix voice installed"
}

# Synthesize one word to trimmed raw 24 kHz mono PCM bytes.
function Get-WordPcm([string]$text) {
    $wav = Join-Path $env:TEMP 'aw_word.wav'
    $synth.SetOutputToWaveFile($wav, $fmt)
    $synth.Speak($text)
    $synth.SetOutputToNull()
    $bytes = [System.IO.File]::ReadAllBytes($wav)
    # Find the data chunk.
    $pos = 12; $pcm = $null
    while ($pos -lt $bytes.Length - 8) {
        $id = [System.Text.Encoding]::ASCII.GetString($bytes, $pos, 4)
        $size = [BitConverter]::ToInt32($bytes, $pos + 4)
        if ($id -eq 'data') { $pcm = New-Object byte[] $size; [Array]::Copy($bytes, $pos + 8, $pcm, 0, $size); break }
        $pos += 8 + $size + ($size % 2)
    }
    if (-not $pcm) { throw "no data chunk for '$text'" }
    Remove-Item -LiteralPath $wav -Force
    # Trim leading/trailing near-silence (|sample| < 220) so words concatenate tightly, keeping a
    # short pad so onsets/offsets are not clipped.
    $sc = $pcm.Length / 2
    $s = New-Object 'System.Int16[]' $sc
    [Buffer]::BlockCopy($pcm, 0, $s, 0, $pcm.Length)
    $first = 0; while ($first -lt $sc -and [Math]::Abs([int]$s[$first]) -lt 220) { $first++ }
    $last = $sc - 1; while ($last -gt $first -and [Math]::Abs([int]$s[$last]) -lt 220) { $last-- }
    if ($first -ge $last) { return ,([byte[]]@()) }
    $pad = 360  # 15 ms at 24 kHz
    $first = [Math]::Max(0, $first - $pad)
    $last = [Math]::Min($sc - 1, $last + $pad)
    $outCount = ($last - $first + 1)
    $out = New-Object byte[] ($outCount * 2)
    [Buffer]::BlockCopy($s, $first * 2, $out, 0, $outCount * 2)
    return ,$out
}

function Build-Bank([string[]]$words, [string]$voice, [string]$lang) {
    $synth.SelectVoice($voice)
    Write-Host "`nBuilding $lang bank with voice: $($synth.Voice.Name)"
    $blob = New-Object System.IO.MemoryStream
    $index = New-Object System.Collections.Generic.List[string]
    foreach ($w in $words) {
        $pcm = Get-WordPcm $w
        $offset = [int]$blob.Length
        $blob.Write($pcm, 0, $pcm.Length)
        $wl = $w.ToLowerInvariant()
        $index.Add(('("{0}", {1}, {2})' -f $wl, $offset, $pcm.Length))
        Write-Host ("  {0}: {1} bytes @ {2}" -f $w, $pcm.Length, $offset)
    }
    $binPath = Join-Path $speechDir "word_bank_$lang.bin"
    [System.IO.File]::WriteAllBytes($binPath, $blob.ToArray())
    Write-Host ("  -> {0} ({1} bytes, {2} words)" -f $binPath, $blob.Length, $words.Count)
    return $index
}

$frVoice = Resolve-Voice $FrenchVoice 'fr'
$enVoice = Resolve-Voice $EnglishVoice 'en'
$frIndex = Build-Bank $frWords $frVoice 'fr'
$enIndex = Build-Bank $enWords $enVoice 'en'
$synth.Dispose()

# ---- Generated Rust index ---------------------------------------------------------------

$rs = @()
$rs += '//! Generated by scripts/gen-word-bank.ps1 - do not edit by hand.'
$rs += '//!'
$rs += '//! The premium word bank index: for each language, `(word, offset, length)` into the'
$rs += '//! packed `word_bank_<lang>.bin` blob of real-voice 24 kHz mono PCM. Regenerate to change'
$rs += '//! the vocabulary or the voice.'
$rs += ''
$rs += '/// French words, spoken by the installed French voice.'
$rs += 'pub static FR_INDEX: &[(&str, u32, u32)] = &['
$frIndex | ForEach-Object { $rs += "    $_," }
$rs += '];'
$rs += ''
$rs += '/// English words, spoken by the installed English voice.'
$rs += 'pub static EN_INDEX: &[(&str, u32, u32)] = &['
$enIndex | ForEach-Object { $rs += "    $_," }
$rs += '];'
$rs += ''
$rsPath = Join-Path $root 'boot/uefi/src/word_bank_gen.rs'
Set-Content -Path $rsPath -Value ($rs -join "`n") -Encoding utf8
Write-Host "`nWrote $rsPath"
Write-Host 'Done. Rebuild the UEFI crate to embed the new word bank.'
