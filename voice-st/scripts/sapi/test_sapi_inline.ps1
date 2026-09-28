try {
  $v = New-Object -ComObject SAPI.SpVoice
  Write-Host "type: $($v.GetType().FullName)"
  Write-Host "alive: $($v -ne $null)"
  $voices = $v.GetVoices()
  Write-Host "voices count: $($voices.Count)"
  for ($i=0; $i -lt $voices.Count; $i++) {
    $tok = $voices.Item($i)
    Write-Host "[$i] $($tok.GetDescription())"
  }
} catch { Write-Host "ERR: $_" }