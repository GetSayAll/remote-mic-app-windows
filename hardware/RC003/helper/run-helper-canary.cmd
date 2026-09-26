@echo off
REM RC003 helper -- ACCEPTANCE ONLY: interception ON plus a canary key.
REM Right-click this file and pick "Run as administrator".
REM
REM Why a canary exists at all: on RC003 the three target keys
REM (Back 0x00F1 / Vol+ 0x0080 / Vol- 0x0081) produce NO Windows events at all,
REM because kbdhid drops those three usages. So "cleared" and "not cleared" look
REM identical from the outside -- the volume never changes either way and no
REM character ever appears. The usual criterion ("no native action happened")
REM is therefore ALWAYS true on RC003: it cannot tell success from failure.
REM
REM This launcher clears Home (usage 0x4A) as well. Windows CAN normally handle
REM Home, so clearing it becomes something you can watch:
REM   1. open Notepad, press Home on the remote -> cursor jumps to line start
REM      (baseline: the key really is reachable by Windows)
REM   2. start this file, wait for the armed state, then press Home -> nothing happens
REM      (proof that clearing reached the report)
REM   3. let it finish (or press Ctrl+C), press Home again -> works again
REM      (proof of fail-open: no leftover clearing, no reboot needed)
REM
REM Bounded to 120 s by default. Pass a number to change it.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

chcp 65001 >nul
set "SAYALL_DUR=120"
if not "%~1"=="" set "SAYALL_DUR=%~1"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-helper.ps1" -Mode run -Duration %SAYALL_DUR% -Canary 0x4A
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
