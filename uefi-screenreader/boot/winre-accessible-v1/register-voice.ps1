# Offline configuration of the mounted WinRE copy (never the running system). Run as SYSTEM.
param([string]$Mount = 'C:\OMNI-BACKUPS\winre\mount', [string]$Log)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path $Log -Force | Out-Null
$clsid = '{76557F36-F953-4AD0-8C8A-08B6257DDCE0}'
$dll = '%SystemDrive%\Program Files\ST\st_sapi.dll'
$voices = @(
    @{ Id = 'ST_FR_SIWIS'; Name = 'ST Siwis'; Desc = 'ST Siwis - Français (neuronal)'; Lang = '40C'; Gender = 'Female'
       Config = '{"backend":"neural","lang":"fr","voice":"ff_siwis"}'; Fallback = '{"backend":"compact","lang":"fr","voice":"female"}' },
    @{ Id = 'ST_EN_HEART'; Name = 'ST Heart'; Desc = 'ST Heart - English (neural)'; Lang = '409'; Gender = 'Female'
       Config = '{"backend":"neural","lang":"en","voice":"af_heart"}'; Fallback = '{"backend":"compact","lang":"en","voice":"female"}' }
)
$loaded = @()
try {
    if (-not (Test-Path "$Mount\Program Files\ST\st_sapi.dll")) { throw 'ST not injected in image' }
    reg load HKLM\WRE_SOFT "$Mount\Windows\System32\config\SOFTWARE" | Out-Null; $loaded += 'HKLM\WRE_SOFT'
    reg load HKLM\WRE_DEF "$Mount\Windows\System32\config\DEFAULT" | Out-Null; $loaded += 'HKLM\WRE_DEF'

    $c = "HKLM:\WRE_SOFT\Classes\CLSID\$clsid"
    New-Item -Force "$c\InprocServer32" | Out-Null
    Set-Item $c -Value 'ST SAPI 5 engine'
    New-ItemProperty "$c\InprocServer32" -Name '(default)' -PropertyType ExpandString -Value $dll -Force | Out-Null
    New-ItemProperty "$c\InprocServer32" -Name 'ThreadingModel' -Value 'Both' -Force | Out-Null
    foreach ($v in $voices) {
        $t = "HKLM:\WRE_SOFT\Microsoft\Speech\Voices\Tokens\$($v.Id)"
        New-Item -Force "$t\Attributes" | Out-Null
        Set-Item $t -Value $v.Desc
        New-ItemProperty $t -Name $v.Lang -Value $v.Desc -Force | Out-Null
        New-ItemProperty $t -Name 'CLSID' -Value $clsid -Force | Out-Null
        New-ItemProperty $t -Name 'STConfig' -Value $v.Config -Force | Out-Null
        New-ItemProperty $t -Name 'STFallback' -Value $v.Fallback -Force | Out-Null
        foreach ($a in @{ Age = 'Adult'; Gender = $v.Gender; Language = $v.Lang; Name = $v.Name; Vendor = 'ST' }.GetEnumerator()) {
            New-ItemProperty "$t\Attributes" -Name $a.Key -Value $a.Value -Force | Out-Null
        }
        Write-Host "[PASS] voice token $($v.Id)"
    }
    # WinRE runs as SYSTEM, whose HKCU is the DEFAULT hive: default SAPI voice = ST Siwis.
    New-Item -Force 'HKLM:\WRE_DEF\Software\Microsoft\Speech\Voices' | Out-Null
    New-ItemProperty 'HKLM:\WRE_DEF\Software\Microsoft\Speech\Voices' -Name 'DefaultTokenId' `
        -Value 'HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech\Voices\Tokens\ST_FR_SIWIS' -Force | Out-Null
    Write-Host '[PASS] default voice = ST_FR_SIWIS'

    # Narrator first, then the recovery environment. The original file is kept.
    $ini = "$Mount\Windows\System32\winpeshl.ini"
    if (-not (Test-Path "$ini.orig")) { Copy-Item $ini "$ini.orig" }
    @(
        '[LaunchApps]'
        '%SYSTEMROOT%\System32\cmd.exe, /c start "" %SYSTEMROOT%\System32\Narrator.exe'
        'X:\sources\recovery\recenv.exe'
    ) | Set-Content $ini -Encoding ascii
    Write-Host '[PASS] winpeshl.ini: Narrator then recenv (original kept as winpeshl.ini.orig)'
} finally {
    [gc]::Collect(); [gc]::WaitForPendingFinalizers()
    foreach ($k in $loaded) { reg unload $k | Out-Null }
    Stop-Transcript | Out-Null
}
