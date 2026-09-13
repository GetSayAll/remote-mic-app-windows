#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Common.ps1')
$ci = Get-SayAllCodeIntegrity
$packages = @(Get-WindowsDriver -Online | Where-Object {
    [IO.Path]::GetFileName($_.OriginalFileName) -ieq 'SayAllThreeButtonFilter.inf'
})
$devices = @(Get-SayAllCollections)
$service = Get-CimInstance Win32_SystemDriver -Filter "Name='SayAllThreeButtonFilter'"
[ordered]@{
    testSigningActive = $ci.testSigningActive
    codeIntegrityOptions = $ci.options
    installedPackageCount = $packages.Count
    publishedInf = @($packages | ForEach-Object { $_.Driver })
    matchedCollectionCount = $devices.Count
    deviceStatus = @($devices | ForEach-Object { $_.Status })
    driverServiceState = $service.State
    physicalMappingVerified = $false
} | ConvertTo-Json
