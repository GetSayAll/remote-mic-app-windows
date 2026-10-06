param([switch]$RealRecycle)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $root 'src-tauri/windows/recycle-retired-files.ps1') -FunctionsOnly
$fixture = Join-Path $root ('target/dev/unified-input/recycle-fixture-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $fixture 'SayAllInput')) | Out-Null
foreach ($relative in @('sayall-hid-host-helper.exe', 'sayall-component-helper.exe', 'SayAllInput/SayAllInput.inf', 'SayAllInput/SayAllInput.sys', 'SayAllInput/SayAllInput.cat', 'settings.json', 'user.txt', 'sayall-helper.exe')) {
    [IO.File]::WriteAllText((Join-Path $fixture $relative), 'test fixture')
}
$candidates = @(Get-SayAllRetiredFiles $fixture)
if ($candidates.Count -ne 5) { throw 'Expected precisely five known retired payload files' }
if (@($candidates | Where-Object { $_ -match 'settings|user.txt|sayall-helper.exe' }).Count) { throw 'Unknown/current file included' }
if (@(Get-SayAllRetiredFiles (Join-Path $fixture 'SayAllInput')).Count -ne 0) { throw 'Nonmatching files included' }
$rejected = $false
try { Get-SayAllRetiredFiles ([IO.Path]::GetPathRoot($fixture)) | Out-Null } catch { $rejected = $true }
if (-not $rejected) { throw 'Volume root accepted' }
$outside = Join-Path $fixture 'outside'
[IO.Directory]::CreateDirectory($outside) | Out-Null
$linked = Join-Path $fixture 'linked'
New-Item -ItemType Junction -Path $linked -Target $outside | Out-Null
$rejected = $false
try { Get-SayAllRetiredFiles $linked | Out-Null } catch { $rejected = $true }
if (-not $rejected) { throw 'Reparse root accepted' }
$productFixture = Join-Path $fixture 'product'
[IO.Directory]::CreateDirectory((Join-Path $productFixture 'licenses')) | Out-Null
$productNames = @('sayall-windows-app.exe', 'sayall-helper.exe', 'frida-gadget.dll', 'uninstall.exe', 'licenses/ATTRIBUTION.md', 'licenses/Frida-COPYING.txt', 'licenses/LICENSE')
foreach ($relative in $productNames + @('settings.json', 'capture-authorization.json', 'user.txt')) {
    [IO.File]::WriteAllText((Join-Path $productFixture $relative), 'product fixture')
}
$productCandidates = @(Get-SayAllRetiredFiles $productFixture -ProductFiles)
if ($productCandidates.Count -ne 7) { throw 'Reinstall cleanup did not select precisely the seven current payload files' }
if (@($productCandidates | Where-Object { $_ -match 'settings|authorization|user.txt' }).Count) { throw 'User state included in reinstall cleanup' }
if ($RealRecycle) {
    Invoke-SayAllRetiredFileRecycle $fixture
    foreach ($candidate in $candidates) { if (Test-Path -LiteralPath $candidate) { throw 'Retired file remained after recycle' } }
    foreach ($name in @('settings.json', 'user.txt', 'sayall-helper.exe')) {
        if ([IO.File]::ReadAllText((Join-Path $fixture $name)) -ne 'test fixture') { throw 'Unrelated file changed' }
    }
    $shell = New-Object -ComObject Shell.Application
    $recycled = @($shell.Namespace(10).Items() | Where-Object { $_.ExtendedProperty('System.Recycle.DeletedFrom') -like "$fixture*" })
    if ($recycled.Count -ne 5) { throw 'Actual Recycle Bin destinations not observed' }
    Invoke-SayAllRetiredFileRecycle $productFixture -ProductFiles
    foreach ($candidate in $productCandidates) { if (Test-Path -LiteralPath $candidate) { throw 'Product file remained after recycle' } }
    foreach ($name in @('settings.json', 'capture-authorization.json', 'user.txt')) {
        if ([IO.File]::ReadAllText((Join-Path $productFixture $name)) -ne 'product fixture') { throw 'User state changed during reinstall cleanup' }
    }
    $recycledProduct = @($shell.Namespace(10).Items() | Where-Object { $_.ExtendedProperty('System.Recycle.DeletedFrom') -like "$productFixture*" })
    if ($recycledProduct.Count -ne 6) { throw 'Product Recycle Bin destinations not observed' }
    Write-Output 'retired-files: 13 checks passed; five retired and six product fixture files observed in Recycle Bin'
} else { Write-Output 'retired-files: 8 checks passed; real recycle deferred' }
# Deliberately retain this tiny isolated fixture until the task evidence cleanup.
