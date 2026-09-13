param([string]$Python = 'python.exe', [switch]$Gadget)
$ErrorActionPreference = 'Stop'
$venv = Join-Path $PSScriptRoot '.venv'
$runtime = Join-Path $venv 'Scripts\python.exe'
$requirements = Join-Path $PSScriptRoot 'requirements.txt'
& $Python -I -c 'import sys,struct; assert sys.version_info >= (3,11) and struct.calcsize("P")==8, "Python >= 3.11 x64 is required"'
if ($LASTEXITCODE -ne 0) { throw 'Unsupported Python runtime.' }
if (-not (Test-Path -LiteralPath $runtime)) {
    & $Python -I -m venv $venv
    if ($LASTEXITCODE -ne 0) { throw 'Virtual environment creation failed.' }
}
& $runtime -I -m pip --isolated --disable-pip-version-check install --index-url https://pypi.org/simple --only-binary=:all: --require-hashes --no-deps -r $requirements
if ($LASTEXITCODE -ne 0) { throw 'Pinned Frida installation failed; no capture was attempted.' }
& $runtime -I -m pip --isolated check
if ($LASTEXITCODE -ne 0) { throw 'Runtime dependency validation failed.' }
& $runtime -I -c 'import frida; assert frida.__version__ == "17.15.3"; print("Pinned Frida runtime ready; no attachment performed.")'
if ($LASTEXITCODE -ne 0) { throw 'Frida import validation failed.' }
if ($Gadget) {
    $assets = Join-Path $PSScriptRoot '.assets'
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    $name = 'frida-gadget-17.15.3-windows-x86_64.dll.xz'
    $archive = Join-Path $assets $name
    if (-not (Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest -Uri ('https://github.com/frida/frida/releases/download/17.15.3/' + $name) -OutFile $archive -TimeoutSec 120
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne 'B566D70189B6D551AD8F4E0BEA24DE08A3D4C0F559BB35B2BDB67D45182240C2') {
        throw 'Gadget archive SHA-256 mismatch; do not load it.'
    }
    Write-Output 'Official Gadget archive verified; not loaded.'
}
