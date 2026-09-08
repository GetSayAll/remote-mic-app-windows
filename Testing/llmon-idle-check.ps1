$ErrorActionPreference = 'Continue'
$wd = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',"$wd\Testing\llmon-idle.log",'-Seconds','16' -WindowStyle Hidden
Start-Sleep -Seconds 16
Write-Host '=== idle 16s log (foreign injections, if any) ==='
Get-Content "$wd\Testing\llmon-idle.log" -ErrorAction SilentlyContinue
