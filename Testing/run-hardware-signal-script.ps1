# 用"模拟硬件信号"驱动应用：在仿真构建上回放 hardware-simulation 仓库导出的
# 信号脚本（按键边沿 + ATVV 语音帧），事件走生产解析链路，因此可以在没有物理
# 遥控器的机器上覆盖 onboarding 向导、按键链路等场景。
#
# 前置：
#   1) 构建仿真可执行文件（一次即可）：
#        cargo build -p sayall-windows-app --features runtime-simulation
#      加 -Release 时用 `--release` 构建。
#   2) 信号脚本由 hardware-simulation 仓库生成：
#        hardware-sim export-app-script <profile.json> <scenario.json> --out <脚本.json>
#      仓库内自带两个手写示例（Testing/hardware-scripts/）。
#
# 行为：
#   - 用隔离状态目录启动应用（向导从第①步开始，不动用户既有状态）；
#   - 应用启动后按脚本时间线回放信号；诊断日志里能看到
#     `hardware_script action=replay|apply ...` 记录；
#   - 结束后打印本次会话内新产生的 hardware_script 日志行。
#
# 用法示例：
#   powershell -NoProfile -ExecutionPolicy Bypass -File Testing\run-hardware-signal-script.ps1 `
#       -ScriptPath Testing\hardware-scripts\hid-buttons-smoke.json -StopExisting
param(
    [Parameter(Mandatory = $true)][string]$ScriptPath,
    [string]$StateDirectory = "",
    [switch]$Release,
    [int]$WaitSeconds = 20,
    [switch]$StopExisting
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$profileDirectory = if ($Release) { "release" } else { "debug" }
$appExecutable = Join-Path $repositoryRoot "target/$profileDirectory/sayall-windows-app.exe"
if (-not (Test-Path -LiteralPath $appExecutable -PathType Leaf)) {
    throw "未找到仿真可执行文件：$appExecutable（先运行 cargo build -p sayall-windows-app --features runtime-simulation）"
}
if (-not (Test-Path -LiteralPath $ScriptPath -PathType Leaf)) {
    throw "信号脚本不存在：$ScriptPath"
}
$scriptFullPath = (Resolve-Path -LiteralPath $ScriptPath).Path

if ([string]::IsNullOrWhiteSpace($StateDirectory)) {
    $StateDirectory = Join-Path ([IO.Path]::GetTempPath()) ("sayall-hardware-sim-" + [Guid]::NewGuid().ToString("N"))
}
New-Item -ItemType Directory -Force -Path $StateDirectory | Out-Null

function Get-RunningInstances {
    Get-Process -Name "sayall-windows-app" -ErrorAction SilentlyContinue
}

if ($StopExisting) {
    $running = Get-RunningInstances
    if ($running) {
        # 走应用自己的优雅退出通道（安装器同款命名事件），不用强杀。
        Add-Type @"
using System;
using System.Runtime.InteropServices;
public class SayAllGracefulExit {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr OpenEventW(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetEvent(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);
}
"@
        $handle = [SayAllGracefulExit]::OpenEventW(0x0002, $false, "Local\SayAll-GracefulExit")
        if ($handle -ne [IntPtr]::Zero) {
            [SayAllGracefulExit]::SetEvent($handle) | Out-Null
            [SayAllGracefulExit]::CloseHandle($handle) | Out-Null
            Write-Host "已请求现有实例优雅退出，等待其结束……"
            $deadline = [DateTime]::UtcNow.AddSeconds(20)
            while ((Get-RunningInstances) -and [DateTime]::UtcNow -lt $deadline) {
                Start-Sleep -Milliseconds 300
            }
        }
        $remaining = Get-RunningInstances
        if ($remaining) {
            throw "现有实例在 20 秒内没有退出（pid: $($remaining.Id -join ', ')）：请先手动退出，或改用独立机器测试"
        }
    }
}

$diagnosticLog = Join-Path $env:LOCALAPPDATA "SayAll\logs\sayall-diagnostic.log"
$logOffset = if (Test-Path -LiteralPath $diagnosticLog) { (Get-Item -LiteralPath $diagnosticLog).Length } else { 0 }

$env:SAYALL_WINDOWS_RUNTIME_SIMULATION = "1"
$env:SAYALL_HARDWARE_SIGNAL_SCRIPT = $scriptFullPath
$env:SAYALL_RUNTIME_SIMULATION_STATE_DIR = $StateDirectory
$env:SAYALL_RUNTIME_SIMULATION_SKIP_EXTERNAL = "1"

Write-Host "可执行文件 : $appExecutable"
Write-Host "信号脚本   : $scriptFullPath"
Write-Host "隔离状态目录: $StateDirectory"
Write-Host "诊断日志   : $diagnosticLog"

$appProcess = Start-Process -FilePath $appExecutable -PassThru
Write-Host "已启动 pid=$($appProcess.Id)；等待 $WaitSeconds 秒后读取诊断日志（应用会保持运行）。"
Start-Sleep -Seconds $WaitSeconds

if (-not (Test-Path -LiteralPath $diagnosticLog)) {
    Write-Warning "未找到诊断日志：$diagnosticLog"
    exit 0
}

$stream = [IO.File]::Open($diagnosticLog, "Open", "Read", "ReadWrite")
try {
    $stream.Seek($logOffset, "Begin") | Out-Null
    $reader = New-Object IO.StreamReader($stream)
    $sessionText = $reader.ReadToEnd()
    $reader.Dispose()
} finally {
    $stream.Dispose()
}

$lines = @($sessionText -split "`n" | Where-Object { $_ -match "hardware_script" } | ForEach-Object { $_.Trim() })
if ($lines.Count -eq 0) {
    Write-Warning "本次会话没有 hardware_script 记录：确认二进制是用 --features runtime-simulation 构建的，且脚本路径正确。"
    exit 1
}

$lines | ForEach-Object { Write-Host $_ }
$applied = @($lines | Where-Object { $_ -match "action=apply" })
$failed = @($applied | Where-Object { $_ -match "terminal_result=failed" })
Write-Host ""
Write-Host "回放完成：应用事件 $($applied.Count) 条，失败 $($failed.Count) 条。"
