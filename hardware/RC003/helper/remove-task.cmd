@echo off
setlocal
cd /d "%~dp0"
title SayAll helper - remove scheduled task

rem ---------------------------------------------------------------
rem  Removes the scheduled task "SayAll RC003 Helper" (revokes the
rem  admin authorization). How to run: RIGHT-CLICK -> "Run as
rem  administrator". ASCII-only on purpose (see install-task.cmd).
rem ---------------------------------------------------------------

echo ================================================================
echo   Removing scheduled task: SayAll RC003 Helper
echo ================================================================
echo.

if not exist "%~dp0target\release\sayall-helper.exe" (
  echo [STOP] helper not built:
  echo        %~dp0target\release\sayall-helper.exe
  echo.
  pause
  exit /b 1
)

"%~dp0target\release\sayall-helper.exe" --remove-task
set "EXITCODE=%ERRORLEVEL%"

echo.
if "%EXITCODE%"=="0" (
  echo   [OK] Task removed. The system is back to the never-authorized
  echo        state: opening the capture switch in the app will ask for
  echo        admin one more time.
) else (
  echo   [FAILED] exit code %EXITCODE%. Did you run as administrator?
)
echo.
pause
