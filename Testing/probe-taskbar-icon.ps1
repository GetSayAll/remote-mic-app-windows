# 任务栏图标跟随探针：抓 Shell_TrayWnd（任务栏本身）并按颜色统计。
#
# 配合 `cargo run -p sayall-windows-app --example taskbar_icon_probe` 使用：
# 探针按 tc=6s 只写 ICON_SMALL（洋红 16px）、tc=14s 写 ICON_BIG+ICON_SMALL（青色）、
# tc=22s 再切回洋红；本脚本在 tc=2/12/20/28s 各抓一次任务栏并统计青色/洋红像素。
#
# 判据：青色只在 tc=20s 那一档跳变（≈+1024 px，一张 32×32 图标），洋红在 tc=12s 与
# tc=28s 跳变、中间一档回落；每次切换后上一档颜色回到背景水平。截图同时落盘供复核。
# 注意：`WM_GETICON` 读回只能证明消息生效（探针 stdout），任务栏是否真的重绘要看本表。
#
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File Testing/probe-taskbar-icon.ps1

param(
    [string]$OutputDirectory = "$env:TEMP\sayall-taskbar-icon-probe",
    [string]$ProbeExecutable = ""
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrayShot {
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string name);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
'@

$repositoryRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ProbeExecutable)) {
    $ProbeExecutable = Join-Path $repositoryRoot 'target\debug\examples\taskbar_icon_probe.exe'
}
if (-not (Test-Path $ProbeExecutable)) {
    throw "找不到探针可执行文件：$ProbeExecutable（先跑 cargo build --example taskbar_icon_probe -p sayall-windows-app）"
}

$tray = [TrayShot]::FindWindow('Shell_TrayWnd', $null)
if ($tray -eq [IntPtr]::Zero) { throw '找不到 Shell_TrayWnd（任务栏）' }

function Get-TrayBitmap {
    $rect = New-Object TrayShot+RECT
    [void][TrayShot]::GetWindowRect($tray, [ref]$rect)
    $bitmap = New-Object System.Drawing.Bitmap ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $hdc = $graphics.GetHdc()
    [void][TrayShot]::PrintWindow($tray, $hdc, 2)
    $graphics.ReleaseHdc($hdc)
    $graphics.Dispose()
    return $bitmap
}

function Measure-Color {
    param([System.Drawing.Bitmap]$Bitmap, [string]$Name)
    $rectangle = New-Object System.Drawing.Rectangle 0, 0, $Bitmap.Width, $Bitmap.Height
    $data = $Bitmap.LockBits($rectangle, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $bytes = New-Object byte[] ($data.Stride * $Bitmap.Height)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
    } finally {
        $Bitmap.UnlockBits($data)
    }
    $cyan = 0
    $magenta = 0
    for ($i = 0; $i -lt $bytes.Length; $i += 4) {
        $b = $bytes[$i]; $g = $bytes[$i + 1]; $r = $bytes[$i + 2]
        if ($r -lt 130 -and $g -gt 160 -and $b -gt 180) { $cyan++ }
        elseif ($r -gt 180 -and $g -lt 130 -and $b -gt 160) { $magenta++ }
    }
    [pscustomobject]@{ Capture = $Name; Cyan = $cyan; Magenta = $magenta }
}

$probe = Start-Process -FilePath $ProbeExecutable -PassThru `
    -RedirectStandardOutput (Join-Path $OutputDirectory 'probe.out.log')
$started = Get-Date
$schedule = @(
    @{ At = 2;  Name = 'tc02-baseline.png' },
    @{ At = 12; Name = 'tc12-small-only-magenta.png' },
    @{ At = 20; Name = 'tc20-big-and-small-cyan.png' },
    @{ At = 28; Name = 'tc28-switched-magenta.png' }
)

$results = @()
foreach ($step in $schedule) {
    $waitMs = ($step.At * 1000) - ((Get-Date) - $started).TotalMilliseconds
    if ($waitMs -gt 0) { Start-Sleep -Milliseconds $waitMs }
    $bitmap = Get-TrayBitmap
    $bitmap.Save((Join-Path $OutputDirectory $step.Name), [System.Drawing.Imaging.ImageFormat]::Png)
    $results += Measure-Color -Bitmap $bitmap -Name $step.Name
    $bitmap.Dispose()
}

if (-not $probe.HasExited) { [void]$probe.WaitForExit(15000) }
if (-not $probe.HasExited) { Stop-Process -Id $probe.Id -Force }

$results | Format-Table -AutoSize | Out-String -Width 120 | Write-Host
Write-Host '--- 探针 stdout ---'
Get-Content (Join-Path $OutputDirectory 'probe.out.log') | Write-Host
Write-Host "截图与探针输出：$OutputDirectory"
