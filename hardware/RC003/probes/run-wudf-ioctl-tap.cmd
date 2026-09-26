@echo off
REM RC003 WUDF IOCTL read-only tap -- right-click "Run as administrator".
REM
REM The probe must run elevated because the target WUDFHost.exe lives in
REM session 0 and a normal user token gets ERROR_ACCESS_DENIED.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-wudf-ioctl-tap.ps1"
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
