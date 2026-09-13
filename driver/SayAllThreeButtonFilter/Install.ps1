#Requires -RunAsAdministrator
param([switch]$CheckOnly)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Common.ps1')
$inf = Join-Path $PSScriptRoot 'SayAllThreeButtonFilter.inf'
Assert-SayAllPackage $PSScriptRoot
$ci = Get-SayAllCodeIntegrity
if (-not $ci.testSigningActive) {
    throw 'Test signing is not active in the running kernel. Use Windows Restart after enabling it.'
}
$conflicts = @(Get-WindowsDriver -Online | Where-Object {
    [IO.Path]::GetFileName($_.OriginalFileName) -ieq 'MiRemoteHidFilter.inf'
})
if ($conflicts.Count) { throw 'RemoteMapper filter already installed. Do not stack both filters.' }
$devices = @(Get-SayAllCollections)
if ($devices.Count -ne 1) { throw "Expected one supported HID collection; found $($devices.Count)." }
Assert-SayAllStopped
if ($CheckOnly) { Write-Output 'PASS: package, signature, kernel, device and process preflight. No changes made.'; return }
Write-Host 'Validated one target collection and package hashes. Installing SayAll three-button filter.'
& pnputil.exe /add-driver $inf /install
$installExitCode = $LASTEXITCODE
if ($installExitCode -notin @(0,3010)) { throw "pnputil failed: $installExitCode" }
$service = Get-CimInstance Win32_SystemDriver -Filter "Name='SayAllThreeButtonFilter'"
if ($service.State -eq 'Running' -and $installExitCode -eq 0) {
    Write-Host 'Driver service is Running. No automatic restart is requested; verify physical key events next.'
} else {
    Write-Host 'Package staged, but driver is not Running. Save work and restart Windows, then run Status.ps1.'
}
Write-Host 'Installation is not hardware verification. Test short presses and voice separately.'

