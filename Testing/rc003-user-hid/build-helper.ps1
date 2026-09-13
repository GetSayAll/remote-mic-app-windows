param([switch]$Prepare)
$ErrorActionPreference = 'Stop'
$venv = Join-Path $PSScriptRoot '.build-venv'
$python = Join-Path $venv 'Scripts\python.exe'
if ($Prepare) {
    if (-not (Test-Path -LiteralPath $python)) {
        & py.exe -3.12 -m venv $venv
        if ($LASTEXITCODE -ne 0) { throw 'Build virtual environment creation failed.' }
    }
    & $python -I -m pip --isolated install --only-binary=:all: --require-hashes --no-deps -r (Join-Path $PSScriptRoot 'requirements-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Pinned build dependencies could not be installed.' }
    & $python -I -m pip check
    if ($LASTEXITCODE -ne 0) { throw 'Build dependency validation failed.' }
}
if (-not (Test-Path -LiteralPath $python)) { throw 'Run build-helper.ps1 -Prepare first.' }
$archive = Join-Path $PSScriptRoot '.assets\frida-gadget-17.15.3-windows-x86_64.dll.xz'
if (-not (Test-Path -LiteralPath $archive)) { throw 'Run prepare.ps1 -Gadget first.' }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne 'B566D70189B6D551AD8F4E0BEA24DE08A3D4C0F559BB35B2BDB67D45182240C2') {
    throw 'Official Gadget archive hash mismatch.'
}
$run = Join-Path $PSScriptRoot ('.build\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$args = @('-I', '-m', 'PyInstaller', '--onedir', '--noconsole', '--name', 'sayall-rc003-helper',
    '--exclude-module', 'frida', '--distpath', (Join-Path $run 'dist'),
    '--workpath', (Join-Path $run 'work'), '--specpath', $run)
foreach ($file in @('hid_tap.js', 'gadget_adapter.js', 'LICENSE-GPL-3.0.txt', 'LICENSE-Frida.txt')) {
    $args += @('--add-data', ((Join-Path $PSScriptRoot $file) + ';.'))
}
$pythonHome = & $python -I -c 'import sys; print(sys.base_prefix)'
if ($LASTEXITCODE -ne 0) { throw 'Python runtime location could not be resolved.' }
$notices = @{
    (Join-Path $pythonHome 'LICENSE.txt') = 'licenses\python'
    (Join-Path $venv 'Lib\site-packages\pyinstaller-6.16.0.dist-info\licenses\COPYING.txt') = 'licenses\pyinstaller'
}
foreach ($notice in $notices.GetEnumerator()) {
    if (-not (Test-Path -LiteralPath $notice.Key -PathType Leaf)) { throw 'A bundled runtime license is missing.' }
    $args += @('--add-data', ($notice.Key + ';' + $notice.Value))
}
$args += @('--add-data', ($archive + ';.assets'))
foreach ($file in @('sayall_bridge.py', 'capture.py', 'gadget_backend.py', 'hid_tap.js', 'gadget_adapter.js', 'README.md', 'requirements-build.txt', 'build-helper.ps1', 'prepare.ps1')) {
    $args += @('--add-data', ((Join-Path $PSScriptRoot $file) + ';source'))
}
$args += Join-Path $PSScriptRoot 'sayall_bridge.py'
New-Item -ItemType Directory -Path $run -Force | Out-Null
$log = Join-Path $run 'build.log'
& $python @args *> $log
$code = $LASTEXITCODE
Get-Content -LiteralPath $log -Tail 12
if ($code -ne 0) { throw 'Standalone helper build failed. See the local build log.' }
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$destination = Join-Path $repo 'src-tauri\binaries\rc003-helper'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$produced = Join-Path $run 'dist\sayall-rc003-helper'
foreach ($entry in Get-ChildItem -LiteralPath $produced) {
    Copy-Item -LiteralPath $entry.FullName -Destination $destination -Recurse -Force
}
Get-FileHash -LiteralPath (Join-Path $destination 'sayall-rc003-helper.exe') -Algorithm SHA256
