[CmdletBinding()]
param([string]$TaskName = "OMNI-Repository-Agent")
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
Write-Host "Removed: $TaskName"
