#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Common.ps1')
Assert-SayAllStopped
$drivers = @(Get-WindowsDriver -Online | Where-Object {
    [IO.Path]::GetFileName($_.OriginalFileName) -ieq 'SayAllThreeButtonFilter.inf'
})
if ($drivers.Count -eq 0) { Write-Host 'SayAll filter is not installed.'; return }
if ($drivers.Count -ne 1) { throw 'Multiple SayAll packages found; inspect the driver store before removing anything.' }
$published = $drivers[0].Driver
if ($published -notmatch '^oem[0-9]+\.inf$') { throw 'Unexpected published INF name' }
Write-Host "Removing only SayAllThreeButtonFilter.inf ($published)."
& pnputil.exe /delete-driver $published /uninstall
if ($LASTEXITCODE -notin @(0,3010)) { throw "Uninstall failed: $LASTEXITCODE" }
Write-Host 'Restart Windows to restore the original HID stack.'
Write-Host 'Test mode and certificate remain unchanged until recovery has been verified.'
