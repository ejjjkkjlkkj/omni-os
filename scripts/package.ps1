# Build a self-contained Windows x64 release: st.exe, st_synth.dll, C header,
# compact engine, and the optional neural backend with a private Python runtime.
# Usage: powershell -File scripts\package.ps1 [-NoNeural]
param(
    [switch]$NoNeural,
    [string]$PythonBase = 'C:\Program Files\Python313',
    [string]$Venv = "$PSScriptRoot\..\work\nextgen-venv"
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot\.."
$version = (Select-String "$root\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "st-$version-windows-x64"
$out = Join-Path $root "release\$name"

Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE) { throw 'cargo build failed' }
    cargo test --release -q
    if ($LASTEXITCODE) { throw 'cargo test failed' }
} finally { Pop-Location }

if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory "$out\include" | Out-Null
Copy-Item "$root\target\release\st.exe", "$root\target\release\st_synth.dll", "$root\target\release\st_synth.dll.lib" $out
Copy-Item "$root\include\st_synth.h" "$out\include"
Copy-Item "$root\README.md", "$root\RELEASE_NOTES.md", "$root\LICENSE-THIRD-PARTY.md" $out
Copy-Item "$root\examples" "$out\examples" -Recurse

if (-not $NoNeural) {
    $neural = "$out\neural"
    New-Item -ItemType Directory "$neural\models", "$neural\python" | Out-Null
    Copy-Item "$root\neural\worker.py", "$root\neural\models.json", "$root\neural\prewarm.json" $neural
    Copy-Item "$root\neural\models\kokoro-v1.0.onnx", "$root\neural\models\voices-v1.0.bin" "$neural\models"
    # Private interpreter: base runtime + stdlib (minus dev/GUI parts) + required packages only.
    $py = "$neural\python"
    Get-ChildItem $PythonBase -File | Where-Object { $_.Extension -in '.exe', '.dll' -and $_.Name -notmatch '^pythonw' } | Copy-Item -Destination $py
    Copy-Item "$PythonBase\DLLs" "$py\DLLs" -Recurse
    robocopy "$PythonBase\Lib" "$py\Lib" /E /NFL /NDL /NJH /NJS /XD site-packages test tests idlelib tkinter turtledemo ensurepip __pycache__ | Out-Null
    # Everything the venv holds except tooling and packages the worker never imports.
    $drop = '^(pip|scipy|scipy\.libs|soundfile|_soundfile.*|cffi|_cffi_backend.*|pycparser)([-.].*)?$'
    New-Item -ItemType Directory "$py\Lib\site-packages" | Out-Null
    Get-ChildItem "$Venv\Lib\site-packages" | Where-Object { $_.Name -notmatch $drop } |
        ForEach-Object { Copy-Item $_.FullName "$py\Lib\site-packages\$($_.Name)" -Recurse }
    Get-ChildItem $py -Recurse -Directory -Filter __pycache__ | Remove-Item -Recurse -Force
    # GPL-3.0 text for phonemizer / eSpeak NG, shipped where users will find it.
    New-Item -ItemType Directory "$out\licenses" | Out-Null
    Copy-Item "$py\Lib\site-packages\phonemizer-*.dist-info\licenses\LICENSE" "$out\licenses\GPL-3.0.txt"
    Copy-Item "$PythonBase\LICENSE.txt" "$out\licenses\Python-PSF.txt" -ErrorAction SilentlyContinue
    # Self-test through the packaged runtime only (no ST_PYTHON / ST_NEURAL_HOME overrides).
    Remove-Item Env:ST_PYTHON, Env:ST_NEURAL_HOME -ErrorAction SilentlyContinue
    & "$out\st.exe" --lang fr --voice ff_siwis --text 'Bonjour, bienvenue dans ST.' --out "$out\demo_fr_neural.wav"
    if ($LASTEXITCODE) { throw 'packaged neural self-test failed (fr)' }
    & "$out\st.exe" --lang en --voice af_heart --text 'Hello, welcome to ST.' --out "$out\demo_en_neural.wav"
    if ($LASTEXITCODE) { throw 'packaged neural self-test failed (en)' }
}
& "$out\st.exe" --lang fr --voice male --text 'Bonjour, bienvenue dans ST.' --out "$out\demo_fr_compact.wav"
if ($LASTEXITCODE) { throw 'packaged compact self-test failed' }
& "$out\st.exe" --lang en --voice female --text 'Hello, welcome to ST.' --out "$out\demo_en_compact.wav"
if ($LASTEXITCODE) { throw 'packaged compact self-test failed' }

# Real C consumer against the packaged DLL (gcc from MSYS2 when available).
$gcc = @('C:\msys64\ucrt64\bin\gcc.exe', 'C:\msys64\mingw64\bin\gcc.exe') | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($gcc) {
    $cbuild = Join-Path ([IO.Path]::GetTempPath()) "st-abi-$version"
    New-Item -ItemType Directory $cbuild -Force | Out-Null
    $env:PATH = "$(Split-Path $gcc);$env:PATH"
    & $gcc -O2 -Wall -Wextra -std=c11 -I "$out\include" "$root\tests\c\abi_test.c" -L $out -lst_synth -o "$cbuild\abi_test.exe"
    if ($LASTEXITCODE) { throw 'C ABI test build failed' }
    Copy-Item "$cbuild\abi_test.exe" $out
    & "$out\abi_test.exe" | Tee-Object "$out\ABI_TEST.txt"
    if ($LASTEXITCODE) { throw 'C ABI test failed (compact)' }
    if (-not $NoNeural) {
        & "$out\abi_test.exe" --neural | Tee-Object "$out\ABI_TEST.txt" -Append
        if ($LASTEXITCODE) { throw 'C ABI test failed (neural)' }
    }
    Remove-Item "$out\abi_test.exe"
} else { Write-Warning 'gcc not found: C ABI test skipped' }

# SBOM (CycloneDX 1.5 JSON, minimal) from Cargo.lock and the private Python site-packages.
$components = @()
$lock = Get-Content "$root\Cargo.lock" -Raw
foreach ($m in [regex]::Matches($lock, '(?m)^name = "([^"]+)"\r?\nversion = "([^"]+)"')) {
    if ($m.Groups[1].Value -ne 'st') { $components += [ordered]@{ type = 'library'; name = $m.Groups[1].Value; version = $m.Groups[2].Value; purl = "pkg:cargo/$($m.Groups[1].Value)@$($m.Groups[2].Value)" } }
}
if (-not $NoNeural) {
    foreach ($d in Get-ChildItem "$out\neural\python\Lib\site-packages" -Directory -Filter '*.dist-info') {
        $meta = Get-Content "$($d.FullName)\METADATA" -TotalCount 40
        $n = ($meta | Select-String '^Name: (.+)$').Matches[0].Groups[1].Value
        $v = ($meta | Select-String '^Version: (.+)$').Matches[0].Groups[1].Value
        $l = ($meta | Select-String '^License(-Expression)?: (.+)$' | Select-Object -First 1)
        $c = [ordered]@{ type = 'library'; name = $n; version = $v; purl = "pkg:pypi/$($n.ToLower())@$v" }
        if ($l) { $c.licenses = @(@{ expression = $l.Matches[0].Groups[2].Value.Trim() }) }
        $components += $c
    }
    foreach ($f in (Get-Content "$root\neural\models.json" -Raw | ConvertFrom-Json).files) {
        $components += [ordered]@{ type = 'machine-learning-model'; name = $f.name; version = '1.0'; licenses = @(@{ license = @{ id = 'Apache-2.0' } }); hashes = @(@{ alg = 'SHA-256'; content = $f.sha256 }) }
    }
}
$commit = (git -C $root rev-parse HEAD).Trim()
[ordered]@{ bomFormat = 'CycloneDX'; specVersion = '1.5'; version = 1
    metadata = [ordered]@{ timestamp = (Get-Date).ToUniversalTime().ToString('o'); component = [ordered]@{ type = 'application'; name = 'st'; version = $version } }
    components = $components } | ConvertTo-Json -Depth 8 | Set-Content "$out\SBOM.cdx.json" -Encoding utf8
[ordered]@{ name = 'st'; version = $version; commit = $commit; dirty = [bool](git -C $root status --porcelain)
    built = (Get-Date).ToUniversalTime().ToString('o'); neural = -not $NoNeural
    abi = @('st_engine_create_v1', 'st_engine_stream_v1', 'st_engine_cancel_v1', 'st_engine_wav_v1', 'st_engine_destroy_v1', 'st_last_error_v1', 'st_free_wav', 'st_synthesize_wav')
    audio = '48 kHz mono; stream float32, WAV PCM24' } | ConvertTo-Json | Set-Content "$out\MANIFEST.json" -Encoding utf8

Get-ChildItem $out -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.FullName.Substring($out.Length + 1).Replace('\', '/')
} | Set-Content "$out\SHA256SUMS.txt" -Encoding utf8
$size = (Get-ChildItem $out -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
"Packaged $name ({0:N0} MB) -> $out" -f $size
