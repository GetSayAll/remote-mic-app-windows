@echo off
setlocal
cd /d "%~dp0"
title SayAll helper - one-time admin setup

rem ---------------------------------------------------------------
rem  One-time setup: registers a scheduled task that lets the main
rem  app start the helper WITHOUT asking for admin again.
rem
rem  How to run: RIGHT-CLICK this file -> "Run as administrator".
rem  Double-clicking runs it WITHOUT elevation; the helper detects
rem  that and stops with a clear message instead of doing nothing.
rem ---------------------------------------------------------------

echo ================================================================
echo   SayAll helper - one-time admin setup
echo ================================================================
echo.
echo This registers a Windows scheduled task named
echo     SayAll RC003 Helper
echo which runs the capture helper WITH admin rights. After this,
echo the main app can start it on demand without any further UAC
echo prompts. The helper exits by itself when the app closes.
echo.

if not exist "%~dp0target\release\sayall-helper.exe" (
  echo [STOP] helper not built:
  echo        %~dp0target\release\sayall-helper.exe
  echo.
  pause
  exit /b 1
)

"%~dp0target\release\sayall-helper.exe" --install-task
set "EXITCODE=%ERRORLEVEL%"

echo.
if "%EXITCODE%"=="0" (
  echo   [OK] Task registered. Next: run trigger-task.cmd to start
  echo        the helper without any admin prompt.
) else (
  echo   [FAILED] exit code %EXITCODE%.
  echo   Most common cause: this window was NOT elevated.
  echo   Right-click install-task.cmd -^> "Run as administrator".
)
echo.
echo You can close this window now.
pause
