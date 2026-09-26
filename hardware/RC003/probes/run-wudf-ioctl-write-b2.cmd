@echo off
REM RC003 write experiment, short re-run for phase B2 -- right-click "Run as administrator".
REM
REM Covers phases A + B2 only (~25 seconds): A is the positive control that must
REM be present, B2 collects the unambiguous F13 evidence that the first full run
REM missed (zero reports passed through the hook in that window).
REM
REM The probe must run elevated because the target WUDFHost.exe lives in
REM session 0 and a normal user token gets ERROR_ACCESS_DENIED.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-wudf-ioctl-write-b2.ps1"
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
