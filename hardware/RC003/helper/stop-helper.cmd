@echo off
rem Kill any stale RC003 helper (pre-rename rc003-helper.exe included).
rem Right-click -> Run as administrator. ASCII only: cmd parses .cmd files as GBK.
chcp 437 >nul

net session >nul 2>&1
if errorlevel 1 (
  echo [DENIED] Please right-click this file and choose "Run as administrator".
  pause
  exit /b 1
)

echo [1/3] Ending scheduled task (best effort)...
schtasks /end /tn "SayAll RC003 Helper" 2>nul
echo [2/3] Killing rc003-helper.exe ...
taskkill /F /IM rc003-helper.exe 2>nul
echo [3/3] Killing sayall-helper.exe ...
taskkill /F /IM sayall-helper.exe 2>nul

echo.
echo [DONE] All helper processes stopped. Close this window.
pause
