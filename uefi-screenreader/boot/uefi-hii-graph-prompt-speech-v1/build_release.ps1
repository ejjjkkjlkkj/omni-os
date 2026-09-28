# Reproducible release build of the UEFI screen reader (SCREENREADER.EFI).
#
#   pwsh -File build_release.ps1 -OutDir <dir> [-FirmwareImage M1603QAAS.308] [-NavBin NAV.BIN]
#
# 1. builds the EFI twice from a clean state and requires identical bytes;
# 2. audits the PE image against the UEFI signing rules (x64 EFI application,
#    NX compatible, 4 KiB section alignment, no section both writable and
#    executable, relocations present);
# 3. writes SHA256SUMS.TXT and BUILD-INFO.json (toolchain versions, source
#    commit, hashes) so anyone can rebuild and compare.
# NAV.BIN (speech for one firmware) is data, never code: it is built separately
# by build_nav_bank.py from the owner's own firmware image, and only its hash
# is recorded here.
param(
    [Parameter(Mandatory)][string]$OutDir,
    [string]$NavBin,
    [string]$Llvm = 'C:\Program Files\LLVM\bin'
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
if (Test-Path $Llvm) { $env:PATH = "$Llvm$([IO.Path]::PathSeparator)$env:PATH" }
$python = if (Get-Command python3 -ErrorAction SilentlyContinue) { 'python3' } else { 'python' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$flags = '--target=x86_64-pc-windows-msvc', '-ffreestanding', '-fshort-wchar', '-fno-stack-protector',
         '-fno-builtin', '-mno-red-zone', '-nostdlib', '-O2', '-Wall', '-Wextra', '-Werror',
         '-DQEV_INTERACTIVE_NAV=1'

function Build([string]$work) {
    New-Item -ItemType Directory -Force $work | Out-Null
    & $python (Join-Path $root generate_units.py) (Join-Path $work speech_units.c) (Join-Path $work speech-unit-metadata.txt) | Out-Null
    if ($LASTEXITCODE) { throw 'generate_units.py failed' }
    # Relative source paths keep absolute build directories out of the object files.
    Push-Location $root
    try {
        clang @flags -c 'hii_graph_prompt_speech_uefi.c' -o (Join-Path $work 'main.obj')
        if ($LASTEXITCODE) { throw 'compile failed' }
        clang @flags -c (Join-Path $work 'speech_units.c') -o (Join-Path $work 'units.obj')
        if ($LASTEXITCODE) { throw 'compile units failed' }
    } finally { Pop-Location }
    lld-link /subsystem:efi_application /entry:efi_main /nodefaultlib /machine:x64 /timestamp:0 /nxcompat `
        "/out:$(Join-Path $work SCREENREADER.EFI)" (Join-Path $work 'main.obj') (Join-Path $work 'units.obj')
    if ($LASTEXITCODE) { throw 'link failed' }
    return (Get-FileHash (Join-Path $work 'SCREENREADER.EFI')).Hash.ToLower()
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) "sr-release-$PID"
$a = Build (Join-Path $tmp 'a')
$b = Build (Join-Path $tmp 'b')
if ($a -ne $b) { throw "build is not reproducible: $a vs $b" }
Write-Host "[PASS] two clean builds are identical: $a"

# PE audit.
$pe = [IO.File]::ReadAllBytes((Join-Path $tmp a SCREENREADER.EFI))
$e = [BitConverter]::ToInt32($pe, 0x3c)
if ([Text.Encoding]::ASCII.GetString($pe, $e, 4) -ne "PE`0`0") { throw 'not a PE image' }
$coff = $e + 4; $opt = $coff + 20
$machine = [BitConverter]::ToUInt16($pe, $coff)
$nsec = [BitConverter]::ToUInt16($pe, $coff + 2)
$optSize = [BitConverter]::ToUInt16($pe, $coff + 16)
$magic = [BitConverter]::ToUInt16($pe, $opt)
$align = [BitConverter]::ToUInt32($pe, $opt + 32)
$subsys = [BitConverter]::ToUInt16($pe, $opt + 68)
$dllch = [BitConverter]::ToUInt16($pe, $opt + 70)
$relocSize = [BitConverter]::ToUInt32($pe, $opt + 112 + 5 * 8 + 4)
if ($machine -ne 0x8664 -or $magic -ne 0x20b) { throw 'not PE32+ x64' }
if ($subsys -ne 10) { throw "subsystem $subsys is not EFI_APPLICATION" }
if (-not ($dllch -band 0x100)) { throw 'NX_COMPAT not set' }
if ($align -ne 4096) { throw "section alignment $align, expected 4096" }
$coffCh = [BitConverter]::ToUInt16($pe, $coff + 18)
if ($coffCh -band 0x0001) { throw 'IMAGE_FILE_RELOCS_STRIPPED: firmware could not relocate the image' }
if (-not ($dllch -band 0x40)) { throw 'DYNAMIC_BASE not set' }
if (-not $relocSize) { throw 'no base relocations' }
$sections = @()
for ($i = 0; $i -lt $nsec; $i++) {
    $s = $opt + $optSize + 40 * $i
    $name = [Text.Encoding]::ASCII.GetString($pe, $s, 8).TrimEnd([char]0)
    $ch = [BitConverter]::ToUInt32($pe, $s + 36)
    if (($ch -band 0x80000000) -and ($ch -band 0x20000000)) { throw "section $name is writable and executable" }
    $sections += [ordered]@{ name = $name; characteristics = ('0x{0:X8}' -f $ch) }
}
Write-Host '[PASS] PE32+ x64 EFI application, NX_COMPAT, DYNAMIC_BASE, 4 KiB alignment, W^X, relocations'

Copy-Item (Join-Path $tmp a SCREENREADER.EFI) (Join-Path $OutDir 'SCREENREADER.EFI') -Force
$sums = @("$a  SCREENREADER.EFI")
$navSha = $null
if ($NavBin) {
    $navSha = (Get-FileHash $NavBin).Hash.ToLower()
    $sums += "$navSha  NAV.BIN"
}
$sums | Set-Content (Join-Path $OutDir 'SHA256SUMS.TXT') -Encoding ascii
$commit = git -C $root rev-parse HEAD
$dirty = [bool](git -C $root status --porcelain -- .)
[ordered]@{
    component = 'accessible-windows UEFI screen reader'
    sourceCommit = $commit
    sourceTreeDirty = $dirty
    efiSha256 = $a
    navBinSha256 = $navSha
    toolchain = [ordered]@{
        clang = ((clang --version) | Select-Object -First 1)
        lldLink = ((lld-link --version) | Select-Object -First 1)
        python = ((& $python --version) 2>&1 | Out-String).Trim()
    }
    flags = $flags -join ' '
    link = '/subsystem:efi_application /entry:efi_main /nodefaultlib /machine:x64 /timestamp:0 /nxcompat'
    pe = [ordered]@{ nxCompat = $true; sectionAlignment = $align; sections = $sections }
} | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $OutDir 'BUILD-INFO.json') -Encoding utf8
Remove-Item $tmp -Recurse -Force
Write-Host "[PASS] release written to $OutDir"
