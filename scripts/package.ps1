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

Get-ChildItem $out -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.FullName.Substring($out.Length + 1).Replace('\', '/')
} | Set-Content "$out\SHA256SUMS.txt" -Encoding utf8
$size = (Get-ChildItem $out -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
"Packaged $name ({0:N0} MB) -> $out" -f $size
