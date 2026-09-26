@echo off
REM RC003 enhanced-capture helper -- interception ON (the real run).
REM Right-click this file and pick "Run as administrator".
REM
REM Clears ONLY the three target usages (Back 0x00F1 / Vol+ 0x0080 / Vol- 0x0081).
REM OK / Home / arrows keep using the native Windows path -- no regression there.
REM Bounded to 300 s, then the hook is removed and native behaviour returns.
REM Pass a number to change the bound, e.g.     run-helper.cmd 60
REM First time here? Use run-helper-observe.cmd instead: same report path, nothing cleared.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

chcp 65001 >nul
set "SAYALL_DUR=300"
if not "%~1"=="" set "SAYALL_DUR=%~1"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-helper.ps1" -Mode run -Duration %SAYALL_DUR%
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
