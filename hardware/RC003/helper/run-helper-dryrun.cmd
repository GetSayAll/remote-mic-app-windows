@echo off
REM RC003 enhanced-capture helper -- DRY RUN.
REM Double-click this file -- NO administrator rights needed (every check is
REM read-only). If you run it elevated anyway, that is harmless.
REM
REM Locates the host carrying RC003, checks that the host is exclusive to RC003,
REM and verifies the Gadget SHA-256. Does NOT inject and does NOT listen.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-helper.ps1" -Mode dryrun
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
