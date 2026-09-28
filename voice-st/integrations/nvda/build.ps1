# Build the NVDA add-on: release\stSynth-<version>.nvda-addon
# The add-on bundles st_synth.dll (compact voices). For neural voices, write the
# path of a full ST release (with neural\) into st_home.txt after installation,
# or set ST_HOME; see integrations\nvda\README.md.
param([string]$StHome)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot\..\.."
$version = (Select-String "$root\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
Push-Location $root
try { cargo build --release; if ($LASTEXITCODE) { throw 'cargo build failed' } } finally { Pop-Location }

$stage = Join-Path ([IO.Path]::GetTempPath()) "stSynth-addon-$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory "$stage\synthDrivers" | Out-Null
(Get-Content "$PSScriptRoot\addon\manifest.ini" -Raw) -replace '(?m)^version = .*$', "version = $version" | Set-Content "$stage\manifest.ini" -Encoding utf8
Copy-Item "$PSScriptRoot\addon\synthDrivers\st.py" "$stage\synthDrivers\"
Copy-Item "$root\target\release\st_synth.dll" "$stage\synthDrivers\"
if ($StHome) { Set-Content "$stage\synthDrivers\st_home.txt" $StHome -Encoding utf8 -NoNewline }

# Validate with the NVDA API stubs and the staged files before packaging.
python "$PSScriptRoot\test_driver.py" "$stage\synthDrivers"
if ($LASTEXITCODE) { throw 'NVDA driver test failed' }

$out = "$root\release\stSynth-$version.nvda-addon"
if (Test-Path $out) { Remove-Item $out }
Compress-Archive -Path "$stage\*" -DestinationPath "$out.zip"
Move-Item "$out.zip" $out
"{0}  {1}" -f (Get-FileHash $out).Hash.ToLower(), (Split-Path $out -Leaf) | Set-Content "$out.sha256"
"Built $out"
