#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$inf = Join-Path $PSScriptRoot 'SayAllThreeButtonFilter.inf'
$manifest = Get-Content (Join-Path $PSScriptRoot 'SHA256.json') -Raw | ConvertFrom-Json
foreach ($entry in $manifest.PSObject.Properties) {
    if ($entry.Name -notmatch '^[a-zA-Z0-9_.-]+$') { throw 'Invalid manifest filename' }
    $actual = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot $entry.Name) -Algorithm SHA256).Hash
    if ($actual -ne $entry.Value) { throw "Package checksum mismatch: $($entry.Name)" }
}
$boot = (& bcdedit.exe /enum '{current}' 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0 -or $boot -notmatch '(?im)^testsigning\s+(Yes|是)\s*$') {
    throw 'Test signing is not enabled. Read README before changing boot settings.'
}
$conflicts = @(Get-WindowsDriver -Online | Where-Object {
    [IO.Path]::GetFileName($_.OriginalFileName) -ieq 'MiRemoteHidFilter.inf'
})
if ($conflicts.Count) { throw 'RemoteMapper filter already installed. Do not stack both filters.' }
$target = 'HID\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4'
$matches = @(Get-PnpDevice -PresentOnly | Where-Object {
    $ids = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds -ErrorAction SilentlyContinue).Data
    $ids -contains $target
})
if ($matches.Count -ne 1) { throw "Expected one supported HID collection; found $($matches.Count)." }
if (Get-Process -Name 'sayall-windows-app' -ErrorAction SilentlyContinue) {
    throw 'Exit SayAll normally before installing. Do not force-kill the Bluetooth session.'
}
Write-Host 'Validated one target collection and package hashes. Installing SayAll three-button filter.'
& pnputil.exe /add-driver $inf /install
if ($LASTEXITCODE -notin @(0,3010)) { throw "pnputil failed: $LASTEXITCODE" }
Write-Host 'Package staged. Restart Windows to attach the filter, then run the local SayAll build.'
Write-Host 'Installation is not hardware verification. Test short presses and voice separately.'

