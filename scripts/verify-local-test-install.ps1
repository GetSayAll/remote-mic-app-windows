# 本地测试包：静默安装 + 静态校验（哈希 / 修订号 / 行为标记）+ 启动截图 + 诊断日志尾部。
#
# 判据（全部用可观察副作用，而不是安装器退出码）：
# - 安装后的 app exe / helper / gadget 与构建产物对齐；app exe 允许 3 字节 UNK→NSS 安装期补丁差；
# - -ExpectRevision 断言内嵌修订号 = 当前 HEAD（bundle 内嵌的是构建时的 git HEAD）；
# - -ExpectAppMarker / -ExpectHelperMarker 断言二进制里含指定行为标记字符串；
# - 被占用文件（如正在运行的 helper）会被安装器静默跳过且退出码仍 0 ⇒ 哈希不符时自动跑第二遍。
#
# 依赖一次成功构建的同仓库产物：target\release\sayall-windows-app.exe、
# src-tauri\sayall-helper.exe（staging 落地）、hardware\RC003\helper\vendor\frida-gadget.dll。
#
# 用法：
#   powershell -File scripts\verify-local-test-install.ps1 -ExpectRevision
#   powershell -File scripts\verify-local-test-install.ps1 -ExpectRevision -ExpectAppMarker voice_gate_sent -ExpectHelperMarker synth:gate
param(
    [string]$Setup = "",
    [switch]$ExpectRevision,
    [string[]]$ExpectAppMarker = @(),
    [string[]]$ExpectHelperMarker = @(),
    [int]$ScreenshotWidth = 1150,
    [int]$ScreenshotHeight = 1500,
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$py = Join-Path $repo "scripts\verify-binary-markers.py"
$failures = @()
function Check([string]$Name, [bool]$Ok, [string]$Detail = "") {
    $tag = if ($Ok) { "PASS" } else { "FAIL"; $script:failures += $Name }
    Write-Host ("  [{0}] {1} {2}" -f $tag, $Name, $Detail)
}

# ---- 1. 找安装包与安装目录 ----
if (-not $Setup) {
    $setupDir = Join-Path $repo "target\release\bundle\nsis"
    $Setup = (Get-ChildItem $setupDir -Filter "*.exe" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}
if (-not $Setup -or -not (Test-Path $Setup)) { throw "找不到安装包：$Setup" }
Write-Host ("SETUP : " + $Setup)

$appDir = Get-ChildItem $env:LOCALAPPDATA -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName "sayall-windows-app.exe") } |
    Select-Object -First 1
if (-not $appDir) { throw "在 %LOCALAPPDATA% 下找不到已安装的 sayall-windows-app.exe" }
Write-Host ("APPDIR: " + $appDir.FullName)

# ---- 2. 静默安装（currentUser，无 UAC） ----
$p = Start-Process -FilePath $Setup -ArgumentList "/S" -Wait -PassThru
Write-Host ("INSTALL_RC=" + $p.ExitCode + "（仅记录；成败以哈希判据为准）")
Start-Sleep -Seconds 8

$instExe = Join-Path $appDir.FullName "sayall-windows-app.exe"
$instHelper = Join-Path $appDir.FullName "sayall-helper.exe"
$instGadget = Join-Path $appDir.FullName "frida-gadget.dll"
$srcExe = Join-Path $repo "target\release\sayall-windows-app.exe"
$srcHelper = Join-Path $repo "src-tauri\sayall-helper.exe"
$srcGadget = Join-Path $repo "hardware\RC003\helper\vendor\frida-gadget.dll"

# ---- 3. 二进制对齐（含被占用文件的重试安装） ----
Write-Host "二进制对齐："
$h1 = & python $py sha $instHelper
$h2 = & python $py sha $srcHelper
if ($h1 -ne $h2) {
    Write-Host "  helper 哈希不符（可能被运行中的助手占用）→ 等 20s 后跑第二遍安装"
    Start-Sleep -Seconds 20
    $p2 = Start-Process -FilePath $Setup -ArgumentList "/S" -Wait -PassThru
    Write-Host ("  INSTALL_RC2=" + $p2.ExitCode)
    Start-Sleep -Seconds 10
    $h1 = & python $py sha $instHelper
}
Check "helper 哈希对齐" ($h1 -eq $h2)
$g1 = & python $py sha $instGadget
$g2 = & python $py sha $srcGadget
Check "gadget 哈希对齐" ($g1 -eq $g2)
$diff = (& python $py diff $instExe $srcExe) | Out-String
$diffOk = $diff -match "differing_bytes=3"
Check "app exe 差 3 字节（UNK→NSS）" $diffOk ($diff.Trim())

# ---- 4. 修订号与行为标记 ----
if ($ExpectRevision) {
    $rev = (& git -C $repo rev-parse HEAD).Trim()
    $cnt = [int](& python $py count $instExe $rev)
    Check ("内嵌修订号 " + $rev.Substring(0, 8)) ($cnt -ge 1) ("count=" + $cnt)
}
foreach ($m in $ExpectAppMarker) {
    $cnt = [int](& python $py count $instExe $m)
    Check ("app 标记 " + $m) ($cnt -ge 1) ("count=" + $cnt)
}
foreach ($m in $ExpectHelperMarker) {
    $cnt = [int](& python $py count $instHelper $m)
    Check ("helper 标记 " + $m) ($cnt -ge 1) ("count=" + $cnt)
}

# ---- 5. 启动 + 窗口截图（PrintWindow，不合成输入） ----
if (-not $NoLaunch) {
    $app = Start-Process -FilePath $instExe -PassThru
    Write-Host ("LAUNCH_PID=" + $app.Id)
    Start-Sleep -Seconds 14
    $app.Refresh()
    $h = $app.MainWindowHandle
    if (-not $h -or $h -eq 0) {
        $app = Get-Process -Id $app.Id -ErrorAction SilentlyContinue
        if ($app) { $app.Refresh(); $h = $app.MainWindowHandle }
    }
    Write-Host ("MAIN_WINDOW_HANDLE=" + $h)
    if ($h -and $h -ne 0) {
        Add-Type -AssemblyName System.Drawing
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class W32 {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int X, int Y, int cx, int cy, uint flags);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
        $r0 = New-Object 'W32+RECT'
        [void][W32]::GetWindowRect($h, [ref]$r0)
        # 临时拉高窗口让整页渲染（非输入注入），拍完还原原尺寸
        [void][W32]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, $ScreenshotWidth, $ScreenshotHeight, 0x0046)
        Start-Sleep -Seconds 4
        $r = New-Object 'W32+RECT'
        [void][W32]::GetWindowRect($h, [ref]$r)
        $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
        $bmp = New-Object System.Drawing.Bitmap($w, $ht)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        $ok = [W32]::PrintWindow($h, $hdc, 2)
        $g.ReleaseHdc($hdc)
        $g.Dispose()
        $png = Join-Path $env:TEMP ("sayall-installed-" + (Get-Date -Format "HHmmss") + ".png")
        $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Check "窗口截图" ([bool]$ok -and (Test-Path $png)) ("-> " + $png)
        [void][W32]::SetWindowPos($h, [IntPtr]::Zero, $r0.Left, $r0.Top, ($r0.Right - $r0.Left), ($r0.Bottom - $r0.Top), 0x0044)
    } else {
        Check "窗口截图" $false "拿不到 MainWindowHandle"
    }
}

# ---- 6. 诊断日志尾部（运行修订号 / 关键链路现场） ----
$diag = Join-Path $env:LOCALAPPDATA "SayAll\Logs\sayall-diagnostic.log"
if (Test-Path $diag) {
    Write-Host "诊断日志尾部："
    Get-Content $diag -Tail 12 | ForEach-Object { Write-Host ("  " + $_) }
} else {
    Write-Host ("（未找到诊断日志：" + $diag + "）")
}

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host ("FAILED：" + ($failures -join "; ")) -ForegroundColor Red
    exit 1
}
Write-Host "PASS：安装校验全部通过"
exit 0
