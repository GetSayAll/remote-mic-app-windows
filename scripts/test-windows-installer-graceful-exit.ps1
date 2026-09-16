$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# 覆盖"应用正在运行时执行安装/卸载"（2026-09-16）。
#
# 为什么必须有这条测试：Tauri 的 NSIS 模板在 `Section Install` 与
# `Section Uninstall` 里，把我们注入的钩子**之后**紧跟 `CheckIfAppIsRunning`，
# 而它做的是 `nsProcess KillProcessCurrentUser` —— 直接 `TerminateProcess`。
# 应用若在持有活动 BLE GATT 会话时被强杀，会留下未关闭的会话并楔死系统蓝牙栈：
# 此后所有 WinRT 入口一律 `0x80070008`，应用内全部自愈手段与睡眠都无效，只能
# 重启电脑（见 Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md）。
#
# 既有的矩阵/静默安装脚本都**没有**覆盖这个场景：它们要么在没有应用运行时安装，
# 要么先 `Start-Process` 再 `Stop-Process -Force`（本身就是那个反模式）。于是这个
# 缺陷在 CI 里长期不可见。本脚本补上这一格，且判据不依赖蓝牙硬件：
#
# 1. 应用启动后创建会话内命名事件并落 `reason=listening` → 证明它"可被请求退出"；
# 2. 安装/卸载开始后该事件被置位 → 日志出现 `reason=installer_requested_exit`；
# 3. 应用自己关会话并落 `ble_session_shutdown ... terminal_result=passed`；
# 4. 进程消失发生在安装器的强杀时刻之前 → 证明是"自己退出"而非"被杀"。
#
# 边界：无蓝牙硬件时会话清理本身是空操作，故本脚本只断言**退出机制**。真机上
# "升级期间不断开正在使用的遥控器"仍需人工验收，见 Testing/WindowsInstallerGracefulExit.md。

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $repositoryRoot "src-tauri/tauri.conf.json"
$bundleDirectory = Join-Path $repositoryRoot "target/release/bundle/nsis"
$config = Get-Content -Raw -Encoding UTF8 -LiteralPath $configPath | ConvertFrom-Json
$productName = $config.productName
$publisher = $config.bundle.publisher
$diagnosticLogPath = Join-Path $env:LOCALAPPDATA "SayAll/Logs/sayall-diagnostic.log"

# 与 crates/sayall-windows/src/graceful_exit.rs 的 GRACEFUL_EXIT_EVENT_NAME 一致；
# src-tauri 的契约测试断言它与 installer-hooks.nsh 中的定义逐字相同。
$gracefulExitEventName = "Local\SayAll-GracefulExit"
# 安装器可能强杀的最早时刻：钩子先固定静默 1.5s，必要时再补 6.5s。
# 与 src-tauri/windows/installer-hooks.nsh 的 SETTLE + TAIL 对应（契约测试守着它）。
$installerGraceMilliseconds = 8000
$listenerReadyMarker = "reason=listening"
$installerRequestedMarker = "reason=installer_requested_exit"
$exitMarkers = @(
    "reason=installer_requested_exit",
    "app_exit ble_session_shutdown phase=completed terminal_result=passed",
    "app_exit platform_shutdown phase=completed terminal_result=passed"
)
$listenerMarkers = @("reason=listening")

$appProcess = $null
$normalUninstallCompleted = $false

function Get-PropertyValue($object, [string] $name) {
    $property = $object.PSObject.Properties[$name]
    if ($null -eq $property) { return $null }
    $property.Value
}

function Get-SayAllUninstallEntries {
    $entries = foreach ($root in @(
            "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
            "HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
        )) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        foreach ($key in Get-ChildItem -LiteralPath $root) {
            $entry = Get-ItemProperty -LiteralPath $key.PSPath
            if (
                (Get-PropertyValue $entry "DisplayName") -eq $productName -and
                (Get-PropertyValue $entry "Publisher") -eq $publisher
            ) { $entry }
        }
    }
    @($entries)
}

function Wait-SingleInstallation {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        $entries = @(Get-SayAllUninstallEntries)
        if ($entries.Count -eq 1) { return $entries }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    $entries
}

function Split-ExecutableCommand([string] $command) {
    $trimmed = $command.Trim()
    if ($trimmed.StartsWith('"')) {
        $closingQuote = $trimmed.IndexOf('"', 1)
        if ($closingQuote -lt 2) { throw "Invalid quoted executable command: $command" }
        return [pscustomobject]@{
            FilePath  = $trimmed.Substring(1, $closingQuote - 1)
            Arguments = $trimmed.Substring($closingQuote + 1).Trim()
        }
    }
    $match = [regex]::Match($trimmed, '^(?<path>.*?\.exe)(?:\s+(?<arguments>.*))?$', 'IgnoreCase')
    if (-not $match.Success) { throw "Command does not contain an executable: $command" }
    [pscustomobject]@{
        FilePath  = $match.Groups['path'].Value
        Arguments = $match.Groups['arguments'].Value.Trim()
    }
}

function Get-SilentUninstallProcess($entry) {
    $command = Get-PropertyValue $entry "QuietUninstallString"
    if ([string]::IsNullOrWhiteSpace($command)) {
        $command = Get-PropertyValue $entry "UninstallString"
    }
    if ([string]::IsNullOrWhiteSpace($command)) {
        throw "SayAll uninstall registry entry has no uninstall command"
    }
    $parts = Split-ExecutableCommand $command
    if (-not (Test-Path -LiteralPath $parts.FilePath -PathType Leaf)) {
        throw "SayAll uninstaller is missing: $($parts.FilePath)"
    }
    $arguments = $parts.Arguments
    if ($arguments -notmatch '(?i)(^|\s)/S($|\s)') { $arguments = "$arguments /S".Trim() }
    Start-Process -FilePath $parts.FilePath -ArgumentList $arguments -PassThru
}

function Wait-ProcessExit($process, [int] $timeoutSeconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($timeoutSeconds)
    while (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
        $process.Refresh()
    }
    $process.HasExited
}

function Invoke-SilentUninstall($entry) {
    $process = Get-SilentUninstallProcess $entry
    if (-not (Wait-ProcessExit $process 180)) {
        throw "Silent SayAll uninstall did not finish in time"
    }
    if ($process.ExitCode -ne 0) {
        throw "Silent SayAll uninstall failed with exit code $($process.ExitCode)"
    }
}

function Get-LogOffset([string] $path) {
    if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-Item -LiteralPath $path).Length } else { 0 }
}

# 只读新增部分：日志跨 CI 步骤持续追加，按偏移量取增量才不会把上一轮的行算进本次。
function Read-LogSince([string] $path, [long] $offset) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return @() }
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        if ($stream.Length -lt $offset) { $offset = 0 }
        $null = $stream.Seek($offset, [IO.SeekOrigin]::Begin)
        $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::UTF8)
        try {
            $text = $reader.ReadToEnd()
        } finally {
            $reader.Dispose()
        }
        return @($text -split "`r?`n" | Where-Object { $_.Length -gt 0 })
    } finally {
        $stream.Dispose()
    }
}

function Test-LogMarkers([string[]] $lines, [string[]] $markers) {
    foreach ($marker in $markers) {
        if (@($lines | Where-Object { $_.Contains($marker) }).Count -eq 0) { return $false }
    }
    $true
}

# 等到给定标记全部出现（或超时），并**返回此刻的快照**：卸载器可能在之后清理
# 应用数据目录，晚读会拿到空文件而误报。
function Wait-LogMarkers([string] $path, [long] $offset, [string[]] $markers, [int] $timeoutSeconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($timeoutSeconds)
    do {
        $lines = @(Read-LogSince $path $offset)
        if (Test-LogMarkers $lines $markers) { return $lines }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    @(Read-LogSince $path $offset)
}

function Assert-LogMarkers([string[]] $lines, [string[]] $markers, [string] $label) {
    foreach ($marker in $markers) {
        if (@($lines | Where-Object { $_.Contains($marker) }).Count -eq 0) {
            $dump = [string]::Join("`n", $lines)
            throw "$label 缺少日志标记 `"$marker`"：应用没有走优雅退出路径，安装器随后会强杀它。`n--- 本次新增日志 ---`n$dump"
        }
    }
}

function Assert-ExitedBeforeInstallerKill($exited, [double] $elapsedMilliseconds, [string] $label) {
    if (-not $exited) {
        throw "$label 期间应用始终没有退出：安装器的优雅退出请求没有生效"
    }
    if ($elapsedMilliseconds -ge $installerGraceMilliseconds) {
        throw ("{0} 期间应用在 {1:N0}ms 后才消失，已达到安装器可能强杀的时刻（{2}ms）：无法排除它是被杀而不是自己退出" -f `
            $label, $elapsedMilliseconds, $installerGraceMilliseconds)
    }
}

function Request-GracefulExit {
    $handle = $null
    try {
        $handle = [System.Threading.EventWaitHandle]::OpenExisting($gracefulExitEventName)
    } catch {
        return $false
    }
    try {
        $null = $handle.Set()
        return $true
    } finally {
        $handle.Dispose()
    }
}

$installers = @(Get-ChildItem -LiteralPath $bundleDirectory -Filter "*-setup.exe" -File)
if ($installers.Count -ne 1) {
    throw "Expected exactly one NSIS installer, found $($installers.Count)"
}
if (@(Get-SayAllUninstallEntries).Count -ne 0) {
    throw "A SayAll installation already exists before the installer graceful-exit test"
}
$installer = $installers[0]

$installElapsedMs = $null
$uninstallElapsedMs = $null

try {
    # ── 场景 1：应用正在运行时执行安装（真实升级路径） ─────────────────────
    $installProcess = Start-Process -FilePath $installer.FullName -ArgumentList "/S" -Wait -PassThru
    if ($installProcess.ExitCode -ne 0) {
        throw "Silent SayAll install failed with exit code $($installProcess.ExitCode)"
    }
    $installEntries = @(Wait-SingleInstallation)
    if ($installEntries.Count -ne 1) {
        throw "Expected one SayAll uninstall entry after install, found $($installEntries.Count)"
    }
    $installLocation = [IO.Path]::GetFullPath((Get-PropertyValue $installEntries[0] "InstallLocation").Trim().Trim('"')).TrimEnd('\')
    $appExecutable = Join-Path $installLocation (Get-PropertyValue $installEntries[0] "MainBinaryName")
    if (-not (Test-Path -LiteralPath $appExecutable -PathType Leaf)) {
        throw "Installed SayAll executable is missing: $appExecutable"
    }

    $installLogOffset = Get-LogOffset $diagnosticLogPath
    $appProcess = Start-Process -FilePath $appExecutable -PassThru
    $readiness = @(Wait-LogMarkers $diagnosticLogPath $installLogOffset $listenerMarkers 60)
    if (-not (Test-LogMarkers $readiness $listenerMarkers)) {
        throw "应用启动后 60s 内没有创建优雅退出监听（缺 `"$listenerReadyMarker`"）：安装器将无法请求它退出"
    }

    $installStartedAt = [DateTime]::UtcNow
    # /UPDATE 走更新器兼容路径：跳过旧卸载器的异步清理，避免与本次安装竞争。
    $overInstall = Start-Process -FilePath $installer.FullName -ArgumentList "/S /UPDATE" -PassThru
    $installExited = Wait-ProcessExit $appProcess 120
    $installElapsedMs = ([DateTime]::UtcNow - $installStartedAt).TotalMilliseconds
    $installLines = @(Wait-LogMarkers $diagnosticLogPath $installLogOffset $exitMarkers 30)
    if (-not (Wait-ProcessExit $overInstall 180)) {
        throw "Over-install did not finish within 180s"
    }
    if ($overInstall.ExitCode -ne 0) {
        throw "Over-install failed with exit code $($overInstall.ExitCode)"
    }

    Assert-LogMarkers $installLines $exitMarkers "安装覆盖运行中的应用"
    Assert-ExitedBeforeInstallerKill $installExited $installElapsedMs "安装覆盖运行中的应用"

    $afterInstallEntries = @(Wait-SingleInstallation)
    if ($afterInstallEntries.Count -ne 1) {
        throw "Expected one SayAll uninstall entry after over-install, found $($afterInstallEntries.Count)"
    }
    if ([Version](Get-PropertyValue $afterInstallEntries[0] "DisplayVersion") -ne [Version]$config.version) {
        throw "Over-install left an unexpected installed version"
    }
    $appProcess = $null

    # ── 场景 2：应用正在运行时执行卸载 ─────────────────────────────────────
    $uninstallLogOffset = Get-LogOffset $diagnosticLogPath
    $appProcess = Start-Process -FilePath $appExecutable -PassThru
    $readiness = @(Wait-LogMarkers $diagnosticLogPath $uninstallLogOffset $listenerMarkers 60)
    if (-not (Test-LogMarkers $readiness $listenerMarkers)) {
        throw "第二次启动后 60s 内没有创建优雅退出监听"
    }

    $uninstallStartedAt = [DateTime]::UtcNow
    $uninstallProcess = Get-SilentUninstallProcess $afterInstallEntries[0]
    $uninstallExited = Wait-ProcessExit $appProcess 120
    $uninstallElapsedMs = ([DateTime]::UtcNow - $uninstallStartedAt).TotalMilliseconds
    $uninstallLines = @(Wait-LogMarkers $diagnosticLogPath $uninstallLogOffset $exitMarkers 30)
    if (-not (Wait-ProcessExit $uninstallProcess 180)) {
        throw "Silent uninstall did not finish within 180s while the app was running"
    }
    if ($uninstallProcess.ExitCode -ne 0) {
        throw "Silent uninstall failed with exit code $($uninstallProcess.ExitCode)"
    }

    Assert-LogMarkers $uninstallLines $exitMarkers "卸载运行中的应用"
    Assert-ExitedBeforeInstallerKill $uninstallExited $uninstallElapsedMs "卸载运行中的应用"
    $appProcess = $null

    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    while (@(Get-SayAllUninstallEntries).Count -ne 0 -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    if (@(Get-SayAllUninstallEntries).Count -ne 0) {
        throw "SayAll uninstall entry remains after the graceful-exit test"
    }
    $normalUninstallCompleted = $true

    Write-Host "Verified installer graceful exit while the app was running: $($installer.Name)"
    Write-Host ("Install over running app: app exited by itself in {0:N0}ms (installer kill needs >= {1}ms)" -f $installElapsedMs, $installerGraceMilliseconds)
    Write-Host ("Uninstall while running: app exited by itself in {0:N0}ms" -f $uninstallElapsedMs)
    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
        @"
### Windows installer graceful exit

- silent install, then `/S /UPDATE` over a **running** app: passed (app exited on its own in $([int]$installElapsedMs)ms; installer force-kill threshold is ${installerGraceMilliseconds}ms)
- `$installerRequestedMarker`: passed
- `app_exit ble_session_shutdown phase=completed terminal_result=passed`: passed
- `/S` uninstall while the app was running: passed (app exited on its own in $([int]$uninstallElapsedMs)ms)
- uninstall entry removed afterwards: passed

This does not validate a real BLE session being closed mid-upgrade (no Bluetooth hardware in CI), visible installer UI, or SmartScreen.
"@ | Add-Content -Encoding UTF8 $env:GITHUB_STEP_SUMMARY
    }
} finally {
    if ($null -ne $appProcess -and -not $appProcess.HasExited) {
        # 绝不在正常路径强杀：强杀会留下未关闭的 GATT 会话。先请求优雅退出，只有
        # 请求无效时才退化为强杀，并留下明确警告。
        if (Request-GracefulExit) {
            $null = Wait-ProcessExit $appProcess 30
        }
        if (-not $appProcess.HasExited) {
            Write-Warning "Graceful exit request did not stop the app; falling back to a forced stop. A real BLE session may now be left open - restart Windows if Bluetooth stops working."
            Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
        }
    }
    if (-not $normalUninstallCompleted) {
        foreach ($remainingEntry in @(Get-SayAllUninstallEntries)) {
            try {
                Invoke-SilentUninstall $remainingEntry
            } catch {
                Write-Warning "Best-effort cleanup failed: $($_.Exception.Message)"
            }
        }
    }
}
