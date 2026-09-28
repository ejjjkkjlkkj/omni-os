param(
    [Parameter(Mandatory=$true)][string]$Voice,
    [Parameter(Mandatory=$true)][string]$Text,
    [Parameter(Mandatory=$true)][string]$Out
)
$ErrorActionPreference = 'Continue'
Write-Host "Params: Voice=$Voice Text=$Text Out=$Out"
$voice = New-Object -ComObject SAPI.SpVoice
Write-Host "voice type: $($voice.GetType().FullName)"
try {
  $voices = $voice.GetVoices()
  Write-Host "voices ok, count=$($voices.Count)"
} catch {
  Write-Host "GetVoices ERR: $_"
  Write-Host "voice is actually: $voice"
  Write-Host "voice.GetType(): $($voice.GetType().FullName)"
  Write-Host "voice.GetVoices -> property?: $($voice | Get-Member -Name GetVoices)"
  exit 1
}