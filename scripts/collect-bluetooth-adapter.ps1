# Collect non-identifying Bluetooth adapter facts for SayAll Windows support.
# Runs as a normal user; elevation is not required.
#
# Output is key=value text. It deliberately excludes adapter name, Bluetooth
# address, device instance ID, driver path, user path, MAC address, serial
# number, and raw event-log messages.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File collect-bluetooth-adapter.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File collect-bluetooth-adapter.ps1 -OutFile adapter-facts.txt
#   powershell -NoProfile -ExecutionPolicy Bypass -File collect-bluetooth-adapter.ps1 -EventWindowDays 14

[CmdletBinding()]
param(
    [int]$EventWindowDays = 7,
    [string]$OutFile
)

$ErrorActionPreference = 'Stop'
$lines = New-Object 'System.Collections.Generic.List[string]'

function Add-Line {
    param([string]$Text)
    $lines.Add($Text)
}

function Get-Token {
    param($Value, [string]$Fallback = 'unknown')
    if ($null -eq $Value) { return $Fallback }
    $text = [string]$Value
    if ($text.Length -eq 0) { return $Fallback }
    return $text
}

function Get-BusToken {
    param([string]$DeviceId)
    if ($DeviceId -match '^(?i)USB\\') { return 'usb' }
    if ($DeviceId -match '^(?i)(PCI|ACPI)\\') { return 'internal' }
    if ($DeviceId -match '^(?i)SDIO\\') { return 'sdio' }
    return 'other'
}

Add-Line ('sayall_bt_adapter_probe version=1 generated_utc=' + (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
Add-Line ('event_window_days=' + [Math]::Abs($EventWindowDays))

# Bluetooth service state only. Names are generic Windows components.
foreach ($serviceName in @('bthserv', 'BthAvctpSvc')) {
    try {
        $service = Get-Service -Name $serviceName -ErrorAction Stop
        Add-Line ('service name=' + $serviceName + ' status=' + (Get-Token $service.Status) + ' start=' + (Get-Token $service.StartType))
    } catch {
        Add-Line ('service name=' + $serviceName + ' status=absent start=absent')
    }
}

# Physical adapters only. Exclude BTHLEDEVICE\*, BTHLE\*, BTH\* stack nodes,
# and USB child interfaces. The instance ID is used locally for matching but
# is never written to the output.
$adapterSource = 'cim'
$adapterQuery = 'ok'
$adapters = @()
try {
    $adapters = @(Get-CimInstance -ClassName Win32_PnPEntity -ErrorAction Stop | Where-Object {
        $_.PNPClass -eq 'Bluetooth' -and $_.DeviceID -match '^(?i)(USB|PCI|ACPI|SDIO)\\'
    })
} catch {
    $adapterQuery = 'error'
}

# Fallback when CIM is unavailable or returns nothing on an unusual image.
if ($adapterQuery -eq 'ok' -and @($adapters).Count -eq 0) {
    try {
        $adapters = @(Get-PnpDevice -Class Bluetooth -ErrorAction Stop | Where-Object {
            $_.InstanceId -match '^(?i)(USB|PCI|ACPI|SDIO)\\'
        } | ForEach-Object {
            [PSCustomObject]@{
                DeviceID = $_.InstanceId
                Status = [string]$_.Status
                ConfigManagerErrorCode = $_.Problem
                Service = 'none'
            }
        })
        $adapterSource = 'pnp'
    } catch {
        $adapterSource = 'pnp'
        $adapterQuery = 'error'
    }
}
Add-Line ('adapter_query=' + $adapterQuery + ' source=' + $adapterSource)
Add-Line ('adapter_count=' + @($adapters).Count)

$index = 0
foreach ($adapter in @($adapters)) {
    $index++
    $deviceId = [string]$adapter.DeviceID
    $bus = Get-BusToken $deviceId
    $status = Get-Token $adapter.Status
    $problem = if ($null -ne $adapter.ConfigManagerErrorCode) { [string]$adapter.ConfigManagerErrorCode } else { 'unknown' }
    $serviceName = Get-Token $adapter.Service 'none'

    $driverVersion = 'unknown'
    try {
        $driver = Get-PnpDeviceProperty -InstanceId $deviceId -KeyName 'DEVPKEY_Device_DriverVersion' -ErrorAction Stop
        $driverVersion = Get-Token $driver.Data
    } catch {
        $driverVersion = 'unknown'
    }

    # MSPower_DeviceEnable is the USB selective-suspend switch exposed by the
    # Windows power framework. It is readable without elevation.
    $selectiveSuspend = 'unknown'
    try {
        $power = Get-CimInstance -Namespace 'root\WMI' -ClassName 'MSPower_DeviceEnable' -ErrorAction Stop |
            Where-Object {
                $_.InstanceName -and $_.InstanceName.StartsWith($deviceId + '_', [StringComparison]::OrdinalIgnoreCase)
            } |
            Select-Object -First 1
        if ($null -ne $power) { $selectiveSuspend = if ($power.Enable) { 'true' } else { 'false' } }
    } catch {
        $selectiveSuspend = 'unknown'
    }

    $wakeArmed = 'unknown'
    try {
        $wake = Get-CimInstance -Namespace 'root\WMI' -ClassName 'MSPower_DeviceWakeEnable' -ErrorAction Stop |
            Where-Object {
                $_.InstanceName -and $_.InstanceName.StartsWith($deviceId + '_', [StringComparison]::OrdinalIgnoreCase)
            } |
            Select-Object -First 1
        if ($null -ne $wake) { $wakeArmed = if ($wake.Enable) { 'true' } else { 'false' } }
    } catch {
        $wakeArmed = 'unknown'
    }

    Add-Line ('adapter index={0} bus={1} status={2} problem={3} service={4} driver_version={5} selective_suspend={6} wake_armed={7}' -f $index, $bus, $status, $problem, $serviceName, $driverVersion, $selectiveSuspend, $wakeArmed)
}

# System events: aggregate provider/id/count only, never message text.
$since = (Get-Date).AddDays(-1 * [Math]::Abs($EventWindowDays))
$events = @()
Add-Line 'event_query_max=20000'
try {
    $events = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; StartTime = $since } -MaxEvents 20000 -ErrorAction Stop)
    Add-Line ('system_event_query=ok system_event_total=' + @($events).Count)
} catch {
    Add-Line 'system_event_query=error error_domain=eventlog reason=system_log_unavailable'
}

$btEvents = @($events | Where-Object { $_.ProviderName -match '(?i)bth|bluetooth' })
Add-Line ('bluetooth_event_total=' + @($btEvents).Count)
$btEvents | Group-Object ProviderName, Id | Sort-Object Count -Descending | Select-Object -First 12 | ForEach-Object {
    $parts = $_.Name -split ', '
    Add-Line ('bluetooth_event provider=' + $parts[0] + ' id=' + $parts[1] + ' count=' + $_.Count)
}

$sleepCount = @($events | Where-Object { $_.ProviderName -eq 'Microsoft-Windows-Kernel-Power' -and $_.Id -eq 42 }).Count
$resumeCount = @($events | Where-Object { $_.ProviderName -eq 'Microsoft-Windows-Kernel-Power' -and $_.Id -eq 107 }).Count
$unexpectedCount = @($events | Where-Object { $_.ProviderName -eq 'Microsoft-Windows-Kernel-Power' -and $_.Id -eq 41 }).Count
Add-Line ('power_event sleep_count=' + $sleepCount + ' resume_count=' + $resumeCount + ' unexpected_shutdown_count=' + $unexpectedCount)

$pnpStartFailure = @($events | Where-Object { $_.ProviderName -eq 'Microsoft-Windows-Kernel-PnP' -and $_.Id -eq 411 }).Count
$pnpDriverLoadFailure = @($events | Where-Object { $_.ProviderName -eq 'Microsoft-Windows-Kernel-PnP' -and $_.Id -eq 219 }).Count
Add-Line ('pnp_event start_failure_count=' + $pnpStartFailure + ' driver_load_failure_count=' + $pnpDriverLoadFailure)

$text = [string]::Join("`r`n", $lines)
$text
if ($OutFile) {
    Set-Content -LiteralPath $OutFile -Value $text -Encoding UTF8
}
