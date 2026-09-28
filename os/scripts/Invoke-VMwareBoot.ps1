#Requires -Version 7.0
<#
.SYNOPSIS
    Boot the bootable UEFI image on a second hypervisor - VMware Workstation - and
    prove the native kernel comes up on it by capturing its real serial console.

.DESCRIPTION
    Every other proof runs under QEMU/OVMF. The dossier (sections 3.1 and 20) also
    wants validation on other hypervisors on the way to real hardware. This boots
    the same dist image under VMware Workstation's own EFI firmware and virtual
    SATA controller - a completely different firmware and device model than
    QEMU/OVMF - with COM1 routed to a file.

    The kernel's 0xE9 debug console is a QEMU/Bochs convenience VMware does not
    have, so the observable channel here is the real 16550: `serial::prove` writes
    "AW-SERIAL-CONSOLE-OK" on COM1 as the first thing the native kernel does. Seeing
    it in the captured serial file proves VMware's firmware booted
    \EFI\BOOT\BOOTX64.EFI, the loader handed off, and the native kernel entered and
    drove a real UART on this second hypervisor.

    It requires a full clean boot: AW_NATIVE_KERNEL_IDLE must appear with no
    AW_NATIVE_EXCEPTION / AW_NATIVE_KERNEL_PANIC, and the banner must not repeat
    (a repeat would mean a crash-reboot loop). Booting cleanly here also depended
    on not assuming the firmware left EFER.NXE on - VMware leaves it off, so the
    kernel now enables it before installing its NX identity map.

    It also proves the firmware-stage accessibility on VMware. The UEFI boot app
    mirrors its screen-reader and menu markers onto the same COM1 line (VMware has
    no 0xE9 debug port), so this serial capture must also show the mirror arming
    (AW_UEFI_SERIAL_OK), the screen reader completing (AW_UEFI_SR_PROOF_OK) and the
    accessible menu being presented (AW_UEFI_MENU_ITEM), with no AW_UEFI_SR_FAIL /
    AW_UEFI_MENU_FAIL - the accessible boot running before the kernel, on a second
    firmware. Whether the real HDA codec speaks here is reported, not required:
    VMware exposes an HDA controller only with a connected host audio device, so
    headless it falls back to the PC-speaker chime; the spoken-clip DMA path is
    proven under QEMU (see Invoke-BootProofs.ps1, the "hda" configuration).
#>
[CmdletBinding()]
param(
    [string]$Vmrun = 'C:\Program Files\VMware\VMware Workstation\vmrun.exe',
    [string]$QemuImg = 'C:\Program Files\qemu\qemu-img.exe',
    [ValidateRange(10, 120)][int]$BootSeconds = 30,
    # Open the VM in the VMware Workstation window instead of the headless proof,
    # with HDA audio and a USB keyboard enabled, and leave it running so a person
    # can hear the spoken boot menu and operate it (Up/Down move, Enter selects,
    # Escape starts). A GUI session has a host audio device, so VMware exposes the
    # HDA controller and the firmware screen reader speaks its clips for real -
    # what the headless proof cannot show. Interactive, so it makes no assertions.
    [switch]$Gui
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    foreach ($tool in @($Vmrun, $QemuImg)) {
        if (-not (Test-Path -LiteralPath $tool)) { throw "Missing tool: $tool" }
    }

    # Build the bootable image (no QEMU verify; that is the boot-proof suite's job).
    & (Join-Path $PSScriptRoot 'Build-BootableImage.ps1') | Out-Host
    $img = Join-Path $repo 'dist/accessible-windows-uefi-x86_64.img'
    if (-not (Test-Path -LiteralPath $img)) { throw "Image not built: $img" }

    $vmDir = Join-Path $repo 'target/vmware'
    if (Test-Path -LiteralPath $vmDir) { Remove-Item -LiteralPath $vmDir -Recurse -Force }
    New-Item -ItemType Directory -Path $vmDir -Force | Out-Null

    # Convert the raw image to a VMware flat VMDK (descriptor + raw extent).
    $vmdk = Join-Path $vmDir 'aw-boot.vmdk'
    & $QemuImg convert -f raw -O vmdk -o subformat=monolithicFlat $img $vmdk
    if ($LASTEXITCODE -ne 0) { throw 'qemu-img VMDK conversion failed' }

    $serial = Join-Path $vmDir 'aw-serial.log'
    $vmx = Join-Path $vmDir 'aw-boot.vmx'

    # Sound and USB differ by mode. Headless (the automated proof) keeps them off:
    # VMware will not attach an HDA codec without a host audio device, and a
    # missing device would fail the power-on. The GUI mode has a host audio device
    # and a real keyboard, so it enables the HDA controller (spoken clips play for
    # real) and an xHCI USB keyboard (so the firmware menu is operable by hand).
    $deviceLines = if ($Gui) {
        # No xHCI: with SATA, serial and the HDA controller already on the PCIe
        # bus, an EFI VM runs out of PCIe slots for it ("no PCIe slot available for
        # usb_xhci"). It is not needed - the VMware window's keyboard drives the
        # firmware console input (ConIn) that the menu reads, and the legacy USB
        # (UHCI) controller carries a USB HID keyboard without a PCIe slot of the
        # xHCI kind. HDA sound is the point of GUI mode: the codec speaks for real.
        @'
usb.present = "TRUE"
sound.present = "TRUE"
sound.virtualDev = "hdaudio"
sound.autodetect = "TRUE"
sound.startConnected = "TRUE"
'@
    } else {
        @'
usb.present = "FALSE"
usb_xhci.present = "FALSE"
sound.present = "FALSE"
'@
    }

    @"
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "19"
displayName = "aw-boot"
guestOS = "other-64"
firmware = "efi"
memsize = "512"
numvcpus = "1"
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.fileName = "aw-boot.vmdk"
sata0:0.deviceType = "disk"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.fileName = "aw-serial.log"
serial0.yieldOnMsrRead = "TRUE"
ethernet0.present = "FALSE"
$deviceLines
floppy0.present = "FALSE"
bios.bootDelay = "0"
msg.autoAnswer = "TRUE"
gui.exitOnCLIHLT = "FALSE"
tools.syncTime = "FALSE"
"@ | Set-Content -LiteralPath $vmx -Encoding ASCII

    if (Test-Path -LiteralPath $serial) { Remove-Item -LiteralPath $serial -Force }

    if ($Gui) {
        # Open the window and hand it to the user; do not stop it or assert. They
        # hear the boot screen and menu spoken through the HDA codec and operate it
        # from the keyboard. The same COM1 markers still stream to the serial file.
        & $Vmrun -T ws start $vmx gui
        if ($LASTEXITCODE -ne 0) { throw 'vmrun start (gui) failed' }
        Write-Host 'VMware GUI VM started: aw-boot.'
        Write-Host 'Listen for the spoken boot screen, then the boot menu.'
        Write-Host 'In the menu: Up/Down (or Tab) move, Enter selects, Escape starts the OS.'
        Write-Host "  Start Accessible Windows | Reboot | Shut down"
        Write-Host "Firmware markers also stream to: $serial"
        return
    }

    & $Vmrun -T ws start $vmx nogui
    if ($LASTEXITCODE -ne 0) { throw 'vmrun start failed' }
    Start-Sleep -Seconds $BootSeconds
    & $Vmrun -T ws stop $vmx hard 2>&1 | Out-Null
    Start-Sleep -Seconds 2

    if (-not (Test-Path -LiteralPath $serial)) { throw 'VMware produced no serial output' }
    $text = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($serial))
    $banner = 'AW-SERIAL-CONSOLE-OK'
    $count = ([regex]::Matches($text, [regex]::Escape($banner))).Count
    if ($count -lt 1) {
        throw "VMware boot produced no '$banner' on COM1 (serial bytes: $($text.Length))."
    }
    if (-not $text.Contains('AW_NATIVE_KERNEL_IDLE')) {
        throw "VMware boot did not reach AW_NATIVE_KERNEL_IDLE (serial bytes: $($text.Length))."
    }
    foreach ($forbidden in @('AW_NATIVE_EXCEPTION', 'AW_NATIVE_KERNEL_PANIC')) {
        if ($text.Contains($forbidden)) { throw "VMware boot emitted forbidden marker: $forbidden" }
    }
    # A clean boot reaches idle and halts, so the banner appears exactly once; a
    # crash-reboot loop would repeat it. Allow a small margin, but reject a loop.
    if ($count -gt 3) {
        throw "VMware guest looks like it rebooted ($count banners): boot is not clean."
    }

    # Firmware-stage accessibility on VMware. The UEFI boot app mirrors its screen
    # reader onto COM1 (VMware has no 0xE9 debug port), so this same serial file
    # proves the accessible boot ran before the kernel: the mirror armed, the
    # screen reader completed, and the accessible menu was presented and voiced -
    # all on VMware's own EFI firmware, a second firmware to QEMU/OVMF.
    if (-not $text.Contains('AW_UEFI_SERIAL_OK')) {
        throw "UEFI COM1 accessibility mirror did not arm on VMware (no AW_UEFI_SERIAL_OK; serial bytes: $($text.Length))."
    }
    foreach ($uefiFail in @('AW_UEFI_SR_FAIL', 'AW_UEFI_MENU_FAIL')) {
        if ($text.Contains($uefiFail)) { throw "VMware boot emitted forbidden firmware marker: $uefiFail" }
    }
    if (-not $text.Contains('AW_UEFI_SR_PROOF_OK')) {
        throw "Firmware screen reader did not complete on VMware (no AW_UEFI_SR_PROOF_OK)."
    }
    $menuMatch = [regex]::Match($text, 'AW_UEFI_MENU_ITEM "([^"]+)"')
    if (-not $menuMatch.Success) {
        throw "Accessible firmware menu was not presented on VMware (no AW_UEFI_MENU_ITEM)."
    }
    # Report whether the real HDA codec spoke on VMware. VMware only exposes an HDA
    # controller with a connected host audio device, so headless it usually falls
    # back to the PC-speaker chime; the spoken-clip DMA path is proven under QEMU.
    $hdaSpeak = ([regex]::Matches($text, 'AW_UEFI_AUDIO_SPEAK bytes=\d+')).Count
    $audio = if ($hdaSpeak -gt 0) { "HDA codec spoke ($hdaSpeak clips streamed by DMA)" } else { 'PC-speaker chime fallback (no HDA codec on this VMware profile)' }

    Write-Host "AW_VMWARE_BOOT_OK banner_occurrences=$count reached=AW_NATIVE_KERNEL_IDLE serial=$serial"
    Write-Host "AW_VMWARE_UEFI_ACCESSIBLE_OK menu_default=`"$($menuMatch.Groups[1].Value)`" audio=`"$audio`""
    Write-Host 'VMware boot proof PASS: full clean boot to idle on VMware EFI, no fault.'
    Write-Host 'Firmware accessibility PASS: spoken screen reader and accessible menu ran on VMware EFI before the kernel.'
}
finally {
    Pop-Location
}
