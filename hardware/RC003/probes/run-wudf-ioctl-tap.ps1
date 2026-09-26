# RC003 WUDF IOCTL read-only tap -- elevated launcher.
#
# Purpose: run the read-only tap probe against the RC003 WUDF host.
# This script must run elevated: the host lives in session 0 and a normal
# user token gets ERROR_ACCESS_DENIED from OpenProcess.
#
# The probe is read-only: it never writes to any target buffer, never calls
# SendInput, never changes a system setting. See wudf_ioctl_tap.py.
#
# Keep this file ASCII-only to avoid encoding corruption on Windows.

$ErrorActionPreference = 'Stop'

$python = 'C:\Users\hd838\.workbuddy\binaries\python\envs\default\Scripts\python.exe'
$probe = Join-Path $PSScriptRoot 'wudf_ioctl_tap.py'
$log = Join-Path (Split-Path $PSScriptRoot -Parent) 'evidence\wudf-ioctl-tap.log'

Write-Host ''
Write-Host '=== RC003 WUDF IOCTL read-only tap ===' -ForegroundColor Cyan
Write-Host 'Read-only: dumps the 9-byte report, does NOT clear it, does NOT inject keys.'
Write-Host ('Log file: ' + $log)
Write-Host ''

if (-not (Test-Path $python)) { Write-Host ('python not found: ' + $python) -ForegroundColor Red; exit 2 }
if (-not (Test-Path $probe)) { Write-Host ('probe not found: ' + $probe) -ForegroundColor Red; exit 2 }

& $python $probe --out $log
$code = $LASTEXITCODE

Write-Host ''
Write-Host ('exit code: ' + $code)
Write-Host 'Done. You can close this window (the log is already on disk).'
Start-Sleep -Seconds 20
exit $code
