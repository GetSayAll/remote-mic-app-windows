# 订阅小米遥控器（RC003）厂商 GATT 服务的全部特征值通知，
# 记录按键报文（返回/音量+/音量- 等不进 Windows HID 栈的按键）。
# 通知处理走 Register-ObjectEvent（PS 事件子系统，跨线程可靠）。
# 用法: probe-rc003-vendor-gatt.ps1 <输出文件> <运行秒数>
param(
    [Parameter(Mandatory = $true)][string]$OutFile,
    [Parameter(Mandatory = $true)][int]$Seconds
)

$script:NotifyLog = $OutFile + '.notify.log'

[Windows.Devices.Bluetooth.BluetoothLEDevice, Windows.Devices.Bluetooth, ContentType = WindowsRuntime] | Out-Null
[Windows.Devices.Bluetooth.GenericAttributeProfile.GattDeviceService, Windows.Devices.Bluetooth, ContentType = WindowsRuntime] | Out-Null
[Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristic, Windows.Devices.Bluetooth, ContentType = WindowsRuntime] | Out-Null
[Windows.Foundation.Collections.IVectorView`1, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
[Windows.Devices.Bluetooth.GenericAttributeProfile.GattCommunicationStatus, Windows.Devices.Bluetooth, ContentType = WindowsRuntime] | Out-Null
[Windows.Foundation.IAsyncOperation`1, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
[Windows.Foundation.IAsyncOperation`2, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
Add-Type -AssemblyName System.Runtime.WindowsRuntime

$null = [System.WindowsRuntimeSystemExtensions]

# IAsyncOperation -> Task 的等待桥
$asTaskOp = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -like 'IAsyncOperation*'
})[0]

function Await($op, $resultType) {
    $task = $asTaskOp.MakeGenericMethod($resultType).Invoke($null, @($op))
    $task.Wait(15000) | Out-Null
    $task.Result
}

# 遥控器 MAC c0:5d:39:c2:8d:91 -> ulong（高位在前）
$addrBytes = [byte[]](0xC0, 0x5D, 0x39, 0xC2, 0x8D, 0x91)
$addr = [UInt64]0
foreach ($b in $addrBytes) { $addr = ($addr -shl 8) -bor $b }

$writer = New-Object System.IO.StreamWriter($OutFile, $true, (New-Object System.Text.UTF8Encoding($false)))
$writer.WriteLine("--- gatt probe start $((Get-Date).ToString('HH:mm:ss')) ---")

$device = Await ([Windows.Devices.Bluetooth.BluetoothLEDevice]::FromBluetoothAddressAsync($addr)) ([Windows.Devices.Bluetooth.BluetoothLEDevice])
if (-not $device) { $writer.WriteLine('connect FAILED'); $writer.Close(); Write-Host 'connect failed'; return }
$writer.WriteLine("connected: $($device.DeviceId)")

[Windows.Devices.Bluetooth.BluetoothCacheMode, Windows.Devices.Bluetooth, ContentType = WindowsRuntime] | Out-Null
$cacheMode = [Windows.Devices.Bluetooth.BluetoothCacheMode]::Uncached

$servicesResult = Await ($device.GetGattServicesAsync($cacheMode)) ([Windows.Devices.Bluetooth.GenericAttributeProfile.GattDeviceServicesResult])
$writer.WriteLine("services: $($servicesResult.Services.Count)")
foreach ($svc in $servicesResult.Services) {
    $writer.WriteLine("service $($svc.Uuid)")
}
$writer.Flush()

# 订阅全部服务的全部可通知特征值
$subCount = 0
foreach ($svc in $servicesResult.Services) {
    $charsResult = Await ($svc.GetCharacteristicsAsync($cacheMode)) ([Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristicsResult])
    foreach ($ch in $charsResult.Characteristics) {
        if ($ch.CharacteristicProperties -band [Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristicProperties]::Indicate -or
            $ch.CharacteristicProperties -band [Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristicProperties]::Notify) {
            $status = Await ($ch.WriteClientCharacteristicConfigurationDescriptorAsync(
                [Windows.Devices.Bluetooth.GenericAttributeProfile.GattClientCharacteristicConfigurationDescriptorValue]::Notify)) ([Windows.Devices.Bluetooth.GenericAttributeProfile.GattCommunicationStatus])
            $svcShort = $svc.Uuid.ToString().Substring(4, 4)
            $chShort = $ch.Uuid.ToString().Substring(4, 4)
            $writer.WriteLine("sub service=$svcShort char=$chShort status=$status")
            if ($status -eq 'Success') {
                $subCount++
                $tag = "$svcShort/$chShort|$NotifyLog"
                $null = Register-ObjectEvent -InputObject $ch -EventName ValueChanged -MessageData $tag -Action {
                    $t = (Get-Date).ToString('HH:mm:ss.fff')
                    $parts = $Event.MessageData -split '\|', 2
                    $tag = $parts[0]
                    $logPath = $parts[1]
                    try {
                        $data = $Event.SourceEventArgs.CharacteristicValue.Data
                        $hex = ($data | ForEach-Object { $_.ToString('X2') }) -join ' '
                        Add-Content -Path $logPath -Value "NOTIFY t=$t char=$tag len=$($data.Length) b=[$hex]" -Encoding UTF8
                    } catch {
                        Add-Content -Path $logPath -Value "NOTIFY_ERR t=$t char=$tag err=$($_.Exception.Message)" -Encoding UTF8
                    }
                }
            }
        }
    }
}
$writer.WriteLine("subscribed_total=$subCount — 现在按遥控器按键")
$writer.Flush()

$deadline = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }

$writer.WriteLine("--- gatt probe end $((Get-Date).ToString('HH:mm:ss')) ---")
$writer.Close()
Write-Host "probe finished -> $OutFile (subscribed $subCount)"
