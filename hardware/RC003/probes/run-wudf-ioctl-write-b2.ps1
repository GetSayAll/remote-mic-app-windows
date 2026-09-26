# RC003 WUDF IOCTL write experiment -- SHORT RE-RUN for the B2 phase only.
#
# Why this exists: in the first full run the B2 window saw ZERO reports pass
# through the hook (no key was pressed in time), so the "rewrite to F13"
# evidence is missing. B2 is the unambiguous half of the proof -- the machine
# has no natural source of VK_F13 -- so it is worth re-collecting.
#
# This run covers phases A + B2 only (about 25 seconds instead of ~75):
#   A  -- positive control (OK key -> VK_RETURN); MUST be included, otherwise
#         this run has no usable observation baseline and the verdict is
#         meaningless.
#   B2 -- rewrite target keys (Back / Vol+ / Vol-) to usage 0x0068 (F13);
#         expect VK_F13 on the Windows side.
#
# Same boundaries as the full run: writes offset 3..8 only, restores on leave,
# no kernel driver, no reboot, no registry, no scheduled task.
#
# Keep this file ASCII-only to avoid encoding corruption on Windows.

$ErrorActionPreference = 'Stop'

$python = 'C:\Users\hd838\.workbuddy\binaries\python\envs\default\Scripts\python.exe'
$probe = Join-Path $PSScriptRoot 'wudf_ioctl_write.py'
$log = Join-Path (Split-Path $PSScriptRoot -Parent) 'evidence\wudf-ioctl-write-b2.log'

Write-Host ''
Write-Host '=== RC003 write experiment: short re-run for phase B2 (A + B2 only) ===' -ForegroundColor Cyan
Write-Host 'Phases: A (control, press OK) then B2 (rewrite target keys to F13).'
Write-Host 'About 25 seconds. No kernel driver, no reboot, no system setting changed.'
Write-Host ('Log file: ' + $log)
Write-Host ''

if (-not (Test-Path $python)) { Write-Host ('python not found: ' + $python) -ForegroundColor Red; exit 2 }
if (-not (Test-Path $probe)) { Write-Host ('probe not found: ' + $probe) -ForegroundColor Red; exit 2 }

& $python $probe --out $log --phases A,B2
$code = $LASTEXITCODE

Write-Host ''
Write-Host ('exit code: ' + $code)
Write-Host 'Done. You can close this window (the log is already on disk).'
Start-Sleep -Seconds 20
exit $code
