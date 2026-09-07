$ErrorActionPreference = 'Stop'
Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','Testing\probe-hookmon.ps1','12' -WindowStyle Hidden
Start-Sleep -Seconds 3
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name F2 -Namespace W2
$np = Get-Process notepad | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
[W2.F2]::SetForegroundWindow($np.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 500
& powershell -NoProfile -ExecutionPolicy Bypass -File "Testing\probe-right-alt.ps1"
Start-Sleep -Seconds 6
Write-Host '--- HOOK LOG ---'
Get-Content "Testing\hookmon.log"
