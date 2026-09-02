$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$appExecutable = Join-Path $repositoryRoot "target/release/sayall-windows-app.exe"
if (-not (Test-Path -LiteralPath $appExecutable -PathType Leaf)) {
    throw "Windows runtime simulation executable is missing: $appExecutable"
}

$reportDirectory = if (-not [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    $env:RUNNER_TEMP
} else {
    [IO.Path]::GetTempPath()
}
$simulationId = [Guid]::NewGuid().ToString('N')
$reportPath = Join-Path $reportDirectory "sayall-runtime-simulation-$simulationId.json"
$stateDirectory = Join-Path $reportDirectory "sayall-runtime-simulation-state-$simulationId"
$env:SAYALL_WINDOWS_RUNTIME_SIMULATION = "1"
$env:SAYALL_RUNTIME_SIMULATION_REPORT = $reportPath
$env:SAYALL_RUNTIME_SIMULATION_STATE_DIR = $stateDirectory
$appProcess = $null

try {
    $appProcess = Start-Process -FilePath $appExecutable -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while (-not $appProcess.HasExited -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
        $appProcess.Refresh()
    }
    if (-not $appProcess.HasExited) {
        throw "Windows Tauri/WebView runtime simulation did not finish within 60 seconds"
    }
    if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
        throw "Windows runtime simulation did not write its report"
    }

    $report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json
    if ($appProcess.ExitCode -ne 0) {
        throw "Windows runtime simulation exited with code $($appProcess.ExitCode): $($report.error)"
    }
    if ($report.passed -ne $true) {
        throw "Windows runtime simulation reported failure: $($report.error)"
    }
    if ($report.platform -ne "windows-ci-simulation") {
        throw "Unexpected runtime simulation platform: $($report.platform)"
    }
    $steps = @($report.steps)
    if ($steps.Count -lt 10) {
        throw "Windows runtime simulation completed only $($steps.Count) verification steps"
    }

    Write-Host "Verified Windows Tauri/WebView runtime simulation: $($steps.Count) steps"
    foreach ($step in $steps) {
        Write-Host "- $step"
    }
    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
        @"
### Windows Tauri/WebView runtime simulation

- actual Windows WebView JavaScript → Tauri IPC → Rust command path: passed
- RC001/RC003 scan and RC001 16 kHz connection state: passed
- explicit simulated audio endpoint and Raw Input state: passed
- mapping persistence and non-injecting SendInput recorder: passed
- five sidebar pages and diagnostics rendering: passed
- first `STREAM_START → 40+80 AUDIO → STREAM_STOP → DRAIN`: 240 samples passed
- disconnect and stop cleanup state: passed

This deterministic simulation does not validate real RC001/RC003 hardware, WinRT BLE notifications, WASAPI sound, Raw Input reports, or real SendInput delivery.
"@ | Add-Content -Encoding UTF8 $env:GITHUB_STEP_SUMMARY
    }
} finally {
    if ($null -ne $appProcess -and -not $appProcess.HasExited) {
        Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
    }
}
