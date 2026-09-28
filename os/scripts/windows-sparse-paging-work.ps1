[CmdletBinding()]
param(
    [string]$WorkRoot = 'C:\AW-SPARSE-WORK',
    [string]$Repo = 'https://github.com/ejjjkkjlkkj/accessible-windows.git',
    [string]$Branch = 'bootstrap-v0.1',
    [string]$CommitMessage = 'Update x86 paging work',
    [switch]$Push
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Invoke-Native {
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,
        [Parameter(ValueFromRemainingArguments)]
        [string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $FilePath $($Arguments -join ' ')"
    }
}

foreach ($command in 'git', 'rustup', 'cargo') {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "Required command not found in PATH: $command"
    }
}

$RepoDir = Join-Path $WorkRoot 'accessible-windows-paging'
New-Item -ItemType Directory -Force -Path $WorkRoot | Out-Null

if (-not (Test-Path (Join-Path $RepoDir '.git'))) {
    if (Test-Path $RepoDir) {
        $existing = Get-ChildItem -Force -LiteralPath $RepoDir -ErrorAction SilentlyContinue
        if ($existing) {
            throw "Target directory already exists and is not a Git checkout: $RepoDir"
        }
    }

    Write-Host '== SPARSE CLONE =='
    Invoke-Native git clone --filter=blob:none --no-checkout --single-branch --branch $Branch $Repo $RepoDir
    Push-Location $RepoDir
    try {
        Invoke-Native git sparse-checkout init --cone
        # The workspace manifest lists the Rust crates explicitly. Fetch only that source subtree;
        # root files such as Cargo.toml, Cargo.lock and rust-toolchain.toml are retained by cone mode.
        Invoke-Native git sparse-checkout set crates
        Invoke-Native git checkout $Branch
    }
    finally {
        Pop-Location
    }
}

Push-Location $RepoDir
try {
    Write-Host '== SYNC BRANCH =='
    Invoke-Native git fetch --filter=blob:none origin $Branch
    Invoke-Native git switch $Branch
    Invoke-Native git pull --ff-only origin $Branch

    Write-Host '== TOOLCHAIN =='
    Invoke-Native rustup toolchain install stable --profile minimal --component rustfmt,clippy
    Invoke-Native rustup default stable
    Invoke-Native rustc -Vv
    Invoke-Native cargo -V

    Write-Host '== FORMAT =='
    Invoke-Native cargo fmt --all
    Invoke-Native cargo fmt --all -- --check

    Write-Host '== PAGING CHECK =='
    Invoke-Native cargo check --locked -p aw-x86-paging --all-targets

    Write-Host '== PAGING TEST =='
    Invoke-Native cargo test --locked -p aw-x86-paging --all-targets

    Write-Host '== PAGING CLIPPY =='
    Invoke-Native cargo clippy --locked -p aw-x86-paging --all-targets -- -D warnings

    Write-Host '== GIT STATUS =='
    Invoke-Native git status --short

    $changes = git status --porcelain -- crates/aw-x86-paging
    if ($LASTEXITCODE -ne 0) {
        throw 'Unable to read Git status.'
    }

    if ([string]::IsNullOrWhiteSpace(($changes -join "`n"))) {
        Write-Host 'NO PAGING CHANGES TO COMMIT'
    }
    else {
        Invoke-Native git add -- crates/aw-x86-paging
        Invoke-Native git diff --cached --check
        Invoke-Native git commit -m $CommitMessage
        Write-Host 'COMMIT = PASS'

        if ($Push) {
            Invoke-Native git push origin $Branch
            Write-Host 'PUSH = PASS'
        }
        else {
            Write-Host 'PUSH = SKIPPED (use -Push to publish the commit)'
        }
    }

    Write-Host '== RESULT =='
    Write-Host 'SPARSE WORKSPACE = PASS'
    Write-Host "WORKDIR = $RepoDir"
    Invoke-Native git log -1 --oneline
}
finally {
    Pop-Location
}
