param([string]$V,[string]$T,[string]$O)
$ErrorActionPreference='Stop'
$sp = New-Object -ComObject SAPI.SpVoice
$cat = $sp.GetVoices()
for($i=0;$i -lt $cat.Count;$i++){if($cat.Item($i).GetDescription() -like "*$V*"){$sp.Voice = $cat.Item($i); break}}
$sp.Volume = 100; $sp.Rate = 0
$fmt = New-Object -ComObject SAPI.SpAudioFormat; $fmt.Type = 21
$st = New-Object -ComObject SAPI.SpFileStream; $st.Format = $fmt; $st.Open($O,3,$false)
$sp.AudioOutputStream = $st
$sp.Speak($T, 0)
$st.Close()
