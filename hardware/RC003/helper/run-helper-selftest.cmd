@echo off
REM RC003 enhanced-capture helper -- built-in SELF-TEST.
REM No administrator rights, no device, no injection.
REM
REM You may simply double-click this file (no UAC prompt appears).
REM 34 checks: SHA-256 vectors, device-name masking, the hand-written JSON
REM extractors, the embedded agent fingerprint, config generation, the
REM vendored-Gadget lock file, the HostPid width contract, a live read-only
REM Enum scan ("every WUDFDiagnosticInfo node must yield a readable HostPid"),
REM the timestamp format, a session-loop deadline regression ("--duration must
REM fire even while an agent holds the connection"), console-handler
REM registration, module enumeration with a positive control (the helper's own
REM process) and a negative control (no Gadget), the Gadget module-name
REM predicate, the DLL placement decision table (reuse vs copy) and the
REM attach/generation/stale-tap plan table, generation-dir naming, and token
REM persistence plus generation-directory reaping.
REM Exit code 0 means all passed.
REM Keep this file ASCII-only to avoid encoding corruption on Windows.

chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-helper.ps1" -Mode selftest
echo.
echo launcher finished with exit code %ERRORLEVEL%
pause
