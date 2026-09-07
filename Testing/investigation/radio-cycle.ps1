# Step 2b: bluetooth radio off/on - attempt WinRT then fallback to PnP device restart
# ASCII only.
$ErrorActionPreference = 'SilentlyContinue'
[void][Windows.Devices.Radio.Radio,Windows.System.Devices,ContentType=WindowsRuntime]

$winrtLoaded = $null -ne ([System.Management.Automation.PSTypeName]'Windows.Devices.Radio.Radio').Type
Write-Output "WINRT_TYPE_LOADED=$winrtLoaded"

if ($winrtLoaded) {
    [void][WindowsRuntimeSystemExtensions]
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    })[0]
    function Await($WinRtTask, $ResultType) {
        $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
        $netTask = $asTask.Invoke($null, @($WinRtTask))
        $netTask.Wait(-1) | Out-Null
        $netTask.Result
    }
    $radios = Await ([Windows.Devices.Radio.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radio.Radio]])
    $bt = $radios | Where-Object { $_.Kind -eq 'Bluetooth' } | Select-Object -First 1
    if ($bt) {
        Write-Output "RADIO=$($bt.Name) STATE=$($bt.State)"
        Write-Output 'RADIO_OFF...'
        $null = Await ($bt.SetStateAsync('Disabled')) ([Windows.Devices.Radio.RadioState])
        Start-Sleep -Seconds 4
        Write-Output 'RADIO_ON...'
        $null = Await ($bt.SetStateAsync('Enabled')) ([Windows.Devices.Radio.RadioState])
        Start-Sleep -Seconds 4
        $radios = Await ([Windows.Devices.Radio.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radio.Radio]])
        $bt = $radios | Where-Object { $_.Kind -eq 'Bluetooth' } | Select-Object -First 1
        Write-Output "RADIO_STATE_AFTER_ON=$($bt.State)"
        Write-Output 'WINRT_DONE'
        exit 0
    }
    Write-Output 'NO_BT_RADIO_FALLBACK_PNP'
}

# Fallback: restart the BT radio PnP device node (public device management API)
$btDev = Get-PnpDevice -Class Bluetooth | Where-Object { $_.FriendlyName -like '*Bluetooth*' -and $_.Status -eq 'OK' -and $_.InstanceId -like 'USB\*' } | Select-Object -First 1
if (-not $btDev) {
    $btDev = Get-PnpDevice -Class Bluetooth | Where-Object { $_.FriendlyName -like '*Intel*Bluetooth*' } | Select-Object -First 1
}
if (-not $btDev) { Write-Output 'NO_PNP_BT_DEVICE'; exit 2 }
Write-Output "PNP_RESTART=$($btDev.FriendlyName) ID=$($btDev.InstanceId)"
Disable-PnpDevice -InstanceId $btDev.InstanceId -Confirm:$false
Start-Sleep -Seconds 5
Enable-PnpDevice -InstanceId $btDev.InstanceId -Confirm:$false
Start-Sleep -Seconds 5
$after = Get-PnpDevice -InstanceId $btDev.InstanceId
Write-Output "PNP_AFTER=$($after.Status)"
Write-Output 'PNP_DONE'
