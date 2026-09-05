# 验证 Register-ObjectEvent + Add-Content 跨线程事件管线是否可靠工作。
# 用 System.Timers.Timer（线程池触发，与 WinRT ValueChanged 同模式）。
$log = "$env:TEMP\event-pipeline-test.log"
Remove-Item $log -ErrorAction SilentlyContinue
$timer = New-Object System.Timers.Timer
$timer.Interval = 300
$null = Register-ObjectEvent -InputObject $timer -EventName Elapsed -MessageData "timer|$log" -Action {
    $t = (Get-Date).ToString('HH:mm:ss.fff')
    $parts = $Event.MessageData -split '\|', 2
    Add-Content -Path $parts[1] -Value "TIMER_EVENT t=$t" -Encoding UTF8
}
$timer.Start()
Start-Sleep -Seconds 2
$timer.Stop()
if (Test-Path $log) { $count = (Get-Content $log).Count; Write-Host "OK: $count timer events logged"; Get-Content $log | Select-Object -First 3 } else { Write-Host 'FAILED: no log file' }
