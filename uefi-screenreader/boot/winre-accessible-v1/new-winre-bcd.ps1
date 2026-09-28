# Standalone BCD store for a Windows RE ramdisk boot where the boot manager, boot.sdi and
# sources\winre.wim are all on the partition the boot manager is started from ("boot").
# Used for the VM test disk and for the OMNI key. Never touches the machine's own BCD.
param([Parameter(Mandatory)][string]$Store, [string]$Locale = 'fr-FR')
$ErrorActionPreference = 'Stop'
function B { $out = bcdedit /store $Store @args; if ($LASTEXITCODE) { throw "bcdedit $args failed: $out" }; $out }
if (Test-Path $Store) { [IO.File]::Delete($Store) }
bcdedit /createstore $Store | Out-Null
B /create '{bootmgr}' /d 'Windows Boot Manager' | Out-Null
B /set '{bootmgr}' device boot | Out-Null
B /set '{bootmgr}' timeout 0 | Out-Null
B /create '{ramdiskoptions}' /d 'Ramdisk' | Out-Null
B /set '{ramdiskoptions}' ramdisksdidevice boot | Out-Null
B /set '{ramdiskoptions}' ramdisksdipath '\boot\boot.sdi' | Out-Null
$g = ((B /create /d 'Windows RE accessible (ST)' /application osloader) -join ' ') -replace '.*(\{[0-9a-f-]+\}).*', '$1'
foreach ($k in 'device', 'osdevice') { B /set $g $k 'ramdisk=[boot]\sources\winre.wim,{ramdiskoptions}' | Out-Null }
B /set $g path '\windows\system32\boot\winload.efi' | Out-Null
B /set $g systemroot '\windows' | Out-Null
B /set $g winpe yes | Out-Null
B /set $g detecthal yes | Out-Null
B /set $g locale $Locale | Out-Null
B /set '{bootmgr}' default $g | Out-Null
B /set '{bootmgr}' displayorder $g | Out-Null
Write-Host "[PASS] BCD $Store entry $g"
