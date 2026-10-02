# Windows runtime simulation（真实 Tauri/WebView + 真实 IPC 旅程）。
#
# 外部入口（设置页「官网」/「GitHub」）会经 opener 打开系统默认浏览器——本机跑会
# 打断操作人（每轮弹 2 次）。因此：**CI 或显式 -IncludeExternalEntries 时执行**，
# 其余本地运行默认跳过并把该步骤如实记为 deferred（不静默丢覆盖面）。
param(
    [switch]$IncludeExternalEntries
)

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
$stdoutPath = Join-Path $reportDirectory "sayall-runtime-simulation-$simulationId.stdout.log"
$stderrPath = Join-Path $reportDirectory "sayall-runtime-simulation-$simulationId.stderr.log"
$env:SAYALL_WINDOWS_RUNTIME_SIMULATION = "1"
$env:SAYALL_RUNTIME_SIMULATION_REPORT = $reportPath
$env:SAYALL_RUNTIME_SIMULATION_STATE_DIR = $stateDirectory
$includeExternal = $IncludeExternalEntries -or ($env:CI -eq "true")
$env:SAYALL_RUNTIME_SIMULATION_SKIP_EXTERNAL = if ($includeExternal) { "0" } else { "1" }
Write-Host ("外部入口（官网/GitHub）: " + $(if ($includeExternal) { "执行（CI 或显式指定）" } else { "跳过（本机运行，避免弹出浏览器）" }))
$appProcess = $null

function Write-SimulationProcessLogs {
    foreach ($path in @($stdoutPath, $stderrPath)) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            Write-Host "Runtime simulation process log: $path"
            Get-Content -Encoding UTF8 -LiteralPath $path | Write-Host
        }
    }
}

try {
    $appProcess = Start-Process -FilePath $appExecutable -PassThru `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while (-not $appProcess.HasExited -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
        $appProcess.Refresh()
    }
    if (-not $appProcess.HasExited) {
        Write-SimulationProcessLogs
        throw "Windows Tauri/WebView runtime simulation did not finish within 60 seconds"
    }
    if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
        Write-SimulationProcessLogs
        throw "Windows runtime simulation did not write its report"
    }

    $report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json
    # PowerShell 5.1 的 `Start-Process -PassThru` + `-RedirectStandardOutput` 组合拿不到
    # 退出码（本机实测：`cmd /c exit 3` 直启读到 3、带重定向读到 $null，PS 5.1.26100）。
    # 退出码不可读时以报告判据为准，不把平台差异误报成仿真失败；可读时仍然强制为零。
    $exitCode = $null
    try { $exitCode = $appProcess.ExitCode } catch { $exitCode = $null }
    $reportError = if ($report.PSObject.Properties.Name -contains "error") { $report.error } else { "" }
    if ($null -eq $exitCode) {
        Write-Warning "runtime simulation 退出码不可读（PowerShell 5.1 重定向限制），按报告判据验收"
    } elseif ($exitCode -ne 0) {
        Write-SimulationProcessLogs
        throw "Windows runtime simulation exited with code ${exitCode}: $reportError"
    }
    if ($report.passed -ne $true) {
        throw "Windows runtime simulation reported failure: $reportError"
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
- four sidebar pages and diagnostics rendering: passed
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
