@echo off
setlocal
title SayAll helper - start on demand

rem ---------------------------------------------------------------
rem  Starts the helper through the scheduled task. NO admin needed.
rem  The helper connects to the running main app and exits by itself
rem  about 20 seconds after the app closes.
rem ---------------------------------------------------------------

echo Starting helper via scheduled task...
schtasks /run /tn "SayAll RC003 Helper"
if errorlevel 1 (
  echo.
  echo   [FAILED] Could not start the task.
  echo   Most likely it is not installed yet.
  echo   Run install-task.cmd as administrator once.
) else (
  echo.
  echo   [OK] Helper is starting.
  echo   Check the main app's buttons page - the bridge status line
  echo   should switch to "connected" within a few seconds.
)
echo.
pause
