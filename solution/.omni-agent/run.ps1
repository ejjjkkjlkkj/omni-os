[CmdletBinding()]
param(
  [switch]$Once,
  [int]$Interval = 300
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Agent = Join-Path $Root ".omni-agent\agent.py"
if (-not (Test-Path $Agent)) { throw "OMNI agent runtime not found: $Agent" }
$args = @($Agent)
if ($Once) { $args += "--once" } else { $args += @("--interval", $Interval) }
& python @args
exit $LASTEXITCODE
