# RC003 WUDF IOCTL write (interception) minimal experiment -- elevated launcher.
#
# Purpose: run the WRITE probe against the RC003 WUDF host and verify whether
# bytes we modify in the 9-byte report are honoured by Windows key translation.
#
# This script must run elevated: the host lives in session 0 and a normal user
# token gets ERROR_ACCESS_DENIED from OpenProcess.
#
# Scope of the change: the probe only writes offset 3..8 of the report buffer
# and writes the original bytes back on the way out. It does NOT install a
# kernel driver, does NOT touch Secure Boot / test signing / driver signing
# policy, does NOT write the registry, does NOT create a scheduled task and
# does NOT require a reboot.
#
# Side effect to be aware of: during the "rewrite" phases the target keys are
# turned into Enter (0x0028) or F13 (0x0068), so please keep the mouse focus on
# the desktop, not inside a text editor.
#
# Keep this file ASCII-only to avoid encoding corruption on Windows.

$ErrorActionPreference = 'Stop'

$python = 'C:\Users\hd838\.workbuddy\binaries\python\envs\default\Scripts\python.exe'
$probe = Join-Path $PSScriptRoot 'wudf_ioctl_write.py'
$log = Join-Path (Split-Path $PSScriptRoot -Parent) 'evidence\wudf-ioctl-write.log'

Write-Host ''
Write-Host '=== RC003 WUDF IOCTL write (interception) experiment ===' -ForegroundColor Cyan
Write-Host 'Writes offset 3..8 of the 9-byte report, restores it on leave.'
Write-Host 'No kernel driver, no reboot, no system setting is changed.'
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
