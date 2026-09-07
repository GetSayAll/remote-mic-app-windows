# 状态采集：应用进程 / 应用安装位置 / 蓝牙无线电（无线麦连接报障排查）
$ErrorActionPreference = 'SilentlyContinue'

Write-Output '=== PROCESS ==='
Get-Process | Where-Object { $_.ProcessName -like '*sayall*' -or $_.ProcessName -like '*remote-mic*' } |
    Select-Object ProcessName, Id, StartTime | Format-Table -AutoSize

Write-Output '=== APP DIRS ==='
foreach ($dir in Get-ChildItem $env:LOCALAPPDATA -Directory) {
    if ($dir.Name -like '*SayAll*' -or $dir.Name -like 'app.getsayall*') {
        Write-Output "DIR: $($dir.FullName)"
        Get-ChildItem $dir.FullName -Recurse -Filter *.exe |
            Select-Object -First 5 | ForEach-Object { Write-Output "EXE: $($_.FullName) $($_.LastWriteTime)" }
    }
}

Write-Output '=== BLUETOOTH RADIO ==='
Get-PnpDevice -Class Bluetooth |
    Select-Object Status, FriendlyName | Format-Table -AutoSize | Out-String -Width 120

Write-Output '=== BLUETOOTH LE DEVICES ==='
Get-PnpDevice | Where-Object { $_.InstanceId -like '*DEV_*' -and $_.Class -eq 'Bluetooth' } |
    Select-Object -First 10 Status, FriendlyName | Format-Table -AutoSize | Out-String -Width 120

Write-Output '=== DIAG LOG ==='
if (Test-Path "$env:USERPROFILE\sayall-diag.log") {
    Write-Output "FOUND $($env:USERPROFILE)\sayall-diag.log"
    Get-Item "$env:USERPROFILE\sayall-diag.log" | Select-Object Length, LastWriteTime | Format-List
} else {
    Write-Output 'NO DIAG LOG'
}
