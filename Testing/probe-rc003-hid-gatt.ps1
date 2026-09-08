# 探针：订阅 RC003 的 HID 服务（0x1812）全部可通知特征值（重点 0x2A4D Report），
# 验证"应用侧 GATT 订阅能否独立于 OS HID 栈收到按键报文"（左键双响应修复的路线 B）。
# 用法: probe-rc003-hid-gatt.ps1 <MAC> <输出文件> <运行秒数>
param(
    [Parameter(Mandatory = $true)][string]$Mac,
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

$asTaskOp = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -like 'IAsyncOperation*'
})[0]

function Await($op, $resultType) {
    $task = $asTaskOp.MakeGenericMethod($resultType).Invoke($null, @($op))
    $task.Wait(15000) | Out-Null
    $task.Result
}

$normalizedMac = ($Mac -replace '[:-]', '').Trim()
if ($normalizedMac -notmatch '^[0-9A-Fa-f]{12}$') {
    throw "MAC 格式无效：请传入 12 位十六进制地址"
}
$addr = [Convert]::ToUInt64($normalizedMac, 16)

$writer = New-Object System.IO.StreamWriter($OutFile, $true, (New-Object System.Text.UTF8Encoding($false)))
$writer.WriteLine("--- hid gatt probe start $((Get-Date).ToString('HH:mm:ss.fff')) ---")

$device = Await ([Windows.Devices.Bluetooth.BluetoothLEDevice]::FromBluetoothAddressAsync($addr)) ([Windows.Devices.Bluetooth.BluetoothLEDevice])
if (-not $device) { $writer.WriteLine('connect FAILED'); $writer.Close(); Write-Host 'connect failed'; return }
$writer.WriteLine("connected: $($device.DeviceId)")

[Windows.Devices.Bluetooth.BluetoothCacheMode, Windows.Devices.Bluetooth, ContentType = WindowsRuntime] | Out-Null
$cacheMode = [Windows.Devices.Bluetooth.BluetoothCacheMode]::Uncached

$servicesResult = Await ($device.GetGattServicesAsync($cacheMode)) ([Windows.Devices.Bluetooth.GenericAttributeProfile.GattDeviceServicesResult])
$writer.WriteLine("services: $($servicesResult.Services.Count)")
$hidService = $null
foreach ($svc in $servicesResult.Services) {
    $writer.WriteLine("service $($svc.Uuid)")
    if ($svc.Uuid -eq '00001812-0000-1000-8000-00805f9b34fb') { $hidService = $svc }
}
if (-not $hidService) { $writer.WriteLine('HID service 0x1812 NOT FOUND'); $writer.Close(); Write-Host 'no hid service'; return }

$charsResult = Await ($hidService.GetCharacteristicsAsync($cacheMode)) ([Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristicsResult])
$writer.WriteLine("hid chars: $($charsResult.Characteristics.Count)")
$subCount = 0
$notifyValue = [Windows.Devices.Bluetooth.GenericAttributeProfile.GattClientCharacteristicConfigurationDescriptorValue]::Notify
$indicateValue = [Windows.Devices.Bluetooth.GenericAttributeProfile.GattClientCharacteristicConfigurationDescriptorValue]::Indicate
$propNotify = [Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristicProperties]::Notify
$propIndicate = [Windows.Devices.Bluetooth.GenericAttributeProfile.GattCharacteristicProperties]::Indicate

foreach ($ch in $charsResult.Characteristics) {
    $chShort = $ch.Uuid.ToString().Substring(4, 4)
    $canNotify = ($ch.CharacteristicProperties -band $propNotify) -ne 0
    $canIndicate = ($ch.CharacteristicProperties -band $propIndicate) -ne 0
    $writer.WriteLine("char $chShort notify=$canNotify indicate=$canIndicate props=$($ch.CharacteristicProperties)")
    if ($canNotify -or $canIndicate) {
        $cccd = if ($canNotify) { $notifyValue } else { $indicateValue }
        $status = Await ($ch.WriteClientCharacteristicConfigurationDescriptorAsync($cccd)) ([Windows.Devices.Bluetooth.GenericAttributeProfile.GattCommunicationStatus])
        $writer.WriteLine("sub char=$chShort cccd=$cccd status=$status")
        if ($status -eq 'Success') {
            $subCount++
            $tag = $chShort
            $null = Register-ObjectEvent -InputObject $ch -EventName ValueChanged -MessageData "$tag|$NotifyLog" -Action {
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
$writer.WriteLine("subscribed_total=$subCount — 现在请按遥控器按键")
$writer.Flush()

$deadline = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }

$writer.WriteLine("--- hid gatt probe end $((Get-Date).ToString('HH:mm:ss.fff')) ---")
$writer.Close()
Write-Host "probe finished -> $OutFile (subscribed $subCount)"
