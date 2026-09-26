@echo off
REM RC003 enhanced-capture helper -- OBSERVE only, nothing is cleared.
REM Right-click this file and pick "Run as administrator".
REM
REM This is the recommended first real-device step: the agent reports the key
REM edges it sees, but never clears a report, so there is zero regression risk
REM for OK / Home / arrows. Use it to confirm the three keys actually reach us
REM before enabling interception with run-helper.cmd.
REM Bounded to 300 s by default. Pass a number to change it, e.g.
REM     run-helper-observe.cmd 60
REM A short run is the easy way to see the --duration auto-finish ([TIMEUP] +
REM [DISARM] + [SUMMARY]) instead of waiting the full 300 s.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

chcp 65001 >nul
set "SAYALL_DUR=300"
if not "%~1"=="" set "SAYALL_DUR=%~1"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-helper.ps1" -Mode observe -Duration %SAYALL_DUR%
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
