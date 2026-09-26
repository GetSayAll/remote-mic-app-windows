@echo off
setlocal
title SayAll helper - task status (detailed)

rem ---------------------------------------------------------------
rem  Shows the full configuration of the scheduled task.
rem  TWO lines matter when the helper cannot open the host process:
rem    1. "Run with highest privileges"  -- must be Yes
rem    2. "Run as user"                  -- which account it runs as
rem  Send the whole output back to the assistant.
rem
rem  NOTE: this file is ASCII-only ON PURPOSE. Chinese text inside a
rem  .cmd gets misparsed when the file is saved as UTF-8 but cmd reads
rem  it as GBK: the rem lines break apart and every fragment then runs
rem  as its own command (hit for real on 2026-09-23).
rem ---------------------------------------------------------------

echo ================================================================
echo   Scheduled task: SayAll RC003 Helper  (detailed)
echo ================================================================
echo.
schtasks /query /tn "SayAll RC003 Helper" /v /fo LIST
echo.
echo ================================================================
echo   Please send the whole output above back to the assistant.
echo   The two lines that matter:
echo     1. Run with highest privileges   -- expect: Yes
echo     2. Run as user                   -- which account
echo ================================================================
echo.
pause
