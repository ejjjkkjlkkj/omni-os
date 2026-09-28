#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateSet("Debug","Release")]
    [string]$Configuration = "Debug",
    [switch]$VerboseCargo
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$KernelRoot = Join-Path (Split-Path $PSScriptRoot -Parent) "kernel\x86_64"
$TargetTriple = "x86_64-unknown-none"

$cargo = (Get-Command cargo -ErrorAction Stop).Source
$rustc = (Get-Command rustc -ErrorAction Stop).Source
$rustup = (Get-Command rustup -ErrorAction Stop).Source

$installed = & $rustup target list --installed
if ($installed -notcontains $TargetTriple) {
    & $rustup target add $TargetTriple
    if ($LASTEXITCODE -ne 0) {
        throw "rustup target add $TargetTriple failed."
    }
}

$sysroot = (& $rustc --print sysroot).Trim()
$rustLld = Join-Path $sysroot "lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe"

if (-not (Test-Path -LiteralPath $rustLld)) {
    $candidate = Get-ChildItem -LiteralPath $sysroot -Recurse -Filter "rust-lld.exe" -File -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $candidate) {
        throw "rust-lld.exe not found in active Rust sysroot."
    }
    $rustLld = $candidate.FullName
}

# Deterministic per-build overrides.
# Cargo environment keys override Cargo TOML config files.
$oldBuildTarget = $env:CARGO_BUILD_TARGET
$oldTargetLinker = $env:CARGO_TARGET_X86_64_UNKNOWN_NONE_LINKER

try {
    $env:CARGO_BUILD_TARGET = $TargetTriple
    $env:CARGO_TARGET_X86_64_UNKNOWN_NONE_LINKER = $rustLld

    $cargoArgs = @("build")
    if ($Configuration -eq "Release") {
        $cargoArgs += "--release"
    }
    if ($VerboseCargo) {
        $cargoArgs += "-vv"
    }

    Push-Location $KernelRoot
    try {
        & $cargo @cargoArgs
        $code = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }

    if ($code -ne 0) {
        throw "Kernel Cargo build failed with exit code $code."
    }

    Write-Host "KERNEL BUILD = PASS"
    Write-Host "TARGET = $TargetTriple"
    Write-Host "LINKER = $rustLld"
    Write-Host "CONFIGURATION = $Configuration"
}
finally {
    $env:CARGO_BUILD_TARGET = $oldBuildTarget
    $env:CARGO_TARGET_X86_64_UNKNOWN_NONE_LINKER = $oldTargetLinker
}