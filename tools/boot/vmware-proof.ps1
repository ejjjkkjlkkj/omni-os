#Requires -Version 7.0
<#
.SYNOPSIS
    Boot omni-os under VMware Workstation (VMware's own EFI firmware and device models) and check
    the whole chain on the real COM1 serial console: firmware protocols, Recovery Core, kernel
    health, the spoken administration session and native speech.

.DESCRIPTION
    Builds the loader and kernel, a GPT disk image (scripts/build_bootable_image.py), converts it
    to VMDK and boots it on a SATA disk, with an NVMe data disk, an HD Audio device and COM1 to
    a file. Every omni-os marker is mirrored on COM1 (VMware has no 0xE9 debug port).

    Required: the loader ran its protocol exerciser with no failure, the Recovery Core booted a
    verified generation, the kernel reached idle with no exception, all protections enforced,
    the administration session proved, native speech rendered. Reported, not required: checks
    that depend on the host (audio needs a host sound device).

    tools/boot/vmware-proof.ps1 [-BootSeconds 90] [-Gui]
#>
[CmdletBinding()]
param(
    [string]$Vmrun = 'C:\Program Files\VMware\VMware Workstation\vmrun.exe',
    [string]$QemuImg = 'C:\Program Files\qemu\qemu-img.exe',
    [ValidateRange(20, 600)][int]$BootSeconds = 90,
    [switch]$Gui
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
$os = Join-Path $root 'os'
$work = Join-Path $root 'build/vmware'
function Invoke-Checked([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') failed ($LASTEXITCODE)" }
}
foreach ($tool in @($Vmrun, $QemuImg)) { if (-not (Test-Path -LiteralPath $tool)) { throw "Missing tool: $tool" } }

# 1. Build the loader and kernel, then a GPT disk with the ESP.
Push-Location $os
try {
    Invoke-Checked cargo @('build', '--locked', '--manifest-path', 'kernel/x86_64/Cargo.toml', '--target', 'x86_64-unknown-none', '--release')
    Invoke-Checked cargo @('build', '--locked', '--manifest-path', 'boot/uefi/Cargo.toml', '--target', 'x86_64-unknown-uefi', '--release')
    $objcopy = Get-ChildItem (& rustc --print sysroot) -Recurse -Filter 'llvm-objcopy.exe' | Select-Object -First 1
    if (-not $objcopy) { throw 'llvm-objcopy not found (rustup component add llvm-tools)' }
    if (Test-Path $work) { Remove-Item $work -Recurse -Force }
    $esp = Join-Path $work 'esp'
    New-Item -ItemType Directory -Force (Join-Path $esp 'EFI/BOOT') | Out-Null
    Copy-Item 'boot/uefi/target/x86_64-unknown-uefi/release/aw-uefi-boot.efi' (Join-Path $esp 'EFI/BOOT/BOOTX64.EFI')
    Invoke-Checked $objcopy.FullName @('-O', 'binary', 'kernel/x86_64/target/x86_64-unknown-none/release/aw-kernel-x86_64', (Join-Path $esp 'KERNEL.BIN'))
    $img = Join-Path $work 'omni-os.img'
    $python = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction Stop)
    Invoke-Checked $python.Source @('scripts/build_bootable_image.py', $img, $esp)
} finally { Pop-Location }

# 2. VMware machine: SATA boot disk, NVMe data disk, HD Audio, COM1 to a file.
$vmdk = Join-Path $work 'omni-os.vmdk'
Invoke-Checked $QemuImg @('convert', '-f', 'raw', '-O', 'vmdk', '-o', 'subformat=monolithicFlat', $img, $vmdk)
$data = Join-Path $work 'data.img'
$stream = [System.IO.File]::Create($data); $stream.SetLength(64MB); $stream.Close()
$dataVmdk = Join-Path $work 'data.vmdk'
Invoke-Checked $QemuImg @('convert', '-f', 'raw', '-O', 'vmdk', '-o', 'subformat=monolithicFlat', $data, $dataVmdk)
$serial = Join-Path $work 'com1.log'
$vmx = Join-Path $work 'omni-os.vmx'
@"
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "19"
displayName = "omni-os proof"
guestOS = "other-64"
firmware = "efi"
memsize = "1024"
numvcpus = "2"
pciBridge0.present = "TRUE"
pciBridge4.present = "TRUE"
pciBridge4.virtualDev = "pcieRootPort"
pciBridge4.functions = "8"
pciBridge5.present = "TRUE"
pciBridge5.virtualDev = "pcieRootPort"
pciBridge5.functions = "8"
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.fileName = "omni-os.vmdk"
sata0:0.deviceType = "disk"
nvme0.present = "TRUE"
nvme0:0.present = "TRUE"
nvme0:0.fileName = "data.vmdk"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.fileName = "com1.log"
serial0.yieldOnMsrRead = "TRUE"
sound.present = "TRUE"
sound.virtualDev = "hdaudio"
sound.autodetect = "TRUE"
sound.startConnected = "TRUE"
usb.present = "TRUE"
ethernet0.present = "FALSE"
floppy0.present = "FALSE"
bios.bootDelay = "0"
msg.autoAnswer = "TRUE"
tools.syncTime = "FALSE"
"@ | Set-Content -LiteralPath $vmx -Encoding ASCII

if ($Gui) {
    Invoke-Checked $Vmrun @('-T', 'ws', 'start', $vmx, 'gui')
    Write-Host "VMware GUI started. Serial: $serial"; return
}
# One boot to the administration session; returns the COM1 text of that boot.
function Invoke-Boot {
    if (Test-Path $serial) { Remove-Item $serial -Force }
    Invoke-Checked $Vmrun @('-T', 'ws', 'start', $vmx, 'nogui')
    $deadline = (Get-Date).AddSeconds($BootSeconds)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 3
        if ((Test-Path $serial) -and ((Get-Content $serial -Raw -ErrorAction SilentlyContinue) -match 'AW_ADMIN_SESSION_BEGIN')) { break }
    }
    Start-Sleep -Seconds 2
    & $Vmrun -T ws stop $vmx hard 2>&1 | Out-Null
    Start-Sleep -Seconds 2
    if (-not (Test-Path $serial)) { throw 'VMware produced no serial output' }
    [System.Text.Encoding]::UTF8.GetString([System.IO.File]::ReadAllBytes($serial))
}
$text = Invoke-Boot

# 3. Verdict.
$failures = [System.Collections.Generic.List[string]]::new()
$required = @(
    'AW_UEFI_SR_PROOF_OK',
    'AW_UEFI_RUNTIME_HANDOFF present=true',
    'AW_RECOVERY_BOOT generation=',
    'AW-SERIAL-CONSOLE-OK',
    'AW_SECURITY_BASELINE_OK',
    'AW_MEMORY_PROTECTION_PROOF_OK',
    'AW_ADMIN_PROOF_OK panels=6',
    'AW_OS_TTS_RENDERED',
    'AW_NATIVE_KERNEL_IDLE',
    'AW_ADMIN_SESSION_BEGIN'
)
foreach ($marker in $required) { if (-not $text.Contains($marker)) { $failures.Add("missing $marker") } }
$protocols = [regex]::Match($text, 'AW_UEFI_PROTOCOLS present=(\d+) exercised=(\d+) marker=(\d+) guarded=(\d+) failed=(\d+) unclassified=(\d+)')
if (-not $protocols.Success) { $failures.Add('no AW_UEFI_PROTOCOLS summary') }
elseif ($protocols.Groups[5].Value -ne '0' -or $protocols.Groups[6].Value -ne '0') {
    $failures.Add("protocol exerciser: $($protocols.Value)")
    foreach ($m in [regex]::Matches($text, 'AW_UEFI_PROTOCOL name=\S+ handles=\d+ use=(failed|unclassified)[^\r\n]*')) { $failures.Add($m.Value) }
}
foreach ($forbidden in @('AW_NATIVE_EXCEPTION', 'AW_NATIVE_KERNEL_PANIC', 'AW_ADMIN_FAIL', 'AW_OS_TTS_FAIL')) {
    if ($text.Contains($forbidden)) { $failures.Add("forbidden $forbidden") }
}
$health = [regex]::Match($text, 'AW_HEALTH_CHECKS[^\r\n]*').Value
foreach ($check in @('kernel', 'storage', 'input', 'accessibility', 'security')) {
    if ($health -notmatch "$check=pass") { $failures.Add("health $check not passing: $health") }
}
Write-Host "VMWARE_PROTOCOLS $($protocols.Value)"
Write-Host "VMWARE_HEALTH $health"
foreach ($pattern in @('AW_LAPIC_MODE[^\r\n]*', 'AW_UEFI_RUNTIME_READY[^\r\n]*', 'AW_NVME_IDENTIFY_OK[^\r\n]*', 'AW_AHCI_PROOF_OK', 'AW_HDA_[A-Z_]*(OK|UNAVAILABLE)[^\r\n]*', 'AW_UEFI_AUDIO_BACKEND[^\r\n]*')) {
    $m = [regex]::Match($text, $pattern); if ($m.Success) { Write-Host "VMWARE_INFO $($m.Value)" }
}
if ($failures.Count -gt 0) {
    $failures | ForEach-Object { Write-Host "VMWARE_FAIL $_" }
    throw "VMware proof failed ($($failures.Count)); serial log: $serial"
}
Write-Host 'VMWARE_PHASE1=PASS (firmware, recovery, kernel, administration session, native speech)'

# Phase 2: update and health promotion on VMware, without a keyboard. The disk carries
# generation 2 and OMNI\UPDATE.REQ; VMware keeps its NVRAM between boots like a real machine.
#   boot 1: first boot, generation 1 becomes known-good
#   boot 2: the request stages generation 2; its kernel records health through UEFI runtime
#           services (mapped mode on VMware)
#   boot 3: the loader verifies that record and promotes generation 2
New-Item -ItemType Directory -Force (Join-Path $esp 'OMNI/GEN/2') | Out-Null
Copy-Item (Join-Path $esp 'KERNEL.BIN') (Join-Path $esp 'OMNI/GEN/2/KERNEL.BIN')
Set-Content -LiteralPath (Join-Path $esp 'OMNI/UPDATE.REQ') -Value 'generation=2' -Encoding ASCII -NoNewline
Push-Location $os
try { Invoke-Checked $python.Source @('scripts/build_bootable_image.py', $img, $esp) } finally { Pop-Location }
Get-ChildItem $work -Filter 'omni-os*.vmdk' | Remove-Item -Force
Invoke-Checked $QemuImg @('convert', '-f', 'raw', '-O', 'vmdk', '-o', 'subformat=monolithicFlat', $img, $vmdk)
$boots = @(
    @{ label = 'first boot'; need = @('AW_RECOVERY_STATE_INIT generation=1 persisted=true') },
    @{ label = 'staged trial'; need = @('AW_UPDATE_STAGED generation=2 tries=2', 'AW_RECOVERY_BOOT generation=2 state=trial_attempt', 'AW_UEFI_RUNTIME_READY mode=mapped', 'AW_HEALTH_RECORDED generation=2') },
    @{ label = 'promotion'; need = @('AW_RECOVERY_PROMOTED generation=2', 'AW_RECOVERY_BOOT generation=2 state=successful') }
)
foreach ($boot in $boots) {
    $log = Invoke-Boot
    foreach ($marker in $boot.need) {
        if (-not $log.Contains($marker)) { throw "VMware $($boot.label): missing $marker (serial: $serial)" }
    }
    Write-Host "VMWARE_PHASE2 $($boot.label): PASS"
}
Write-Host 'OMNI_OS_VMWARE=PASS'
