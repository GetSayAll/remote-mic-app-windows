# 按窗口标题截取应用窗口（自测截图用）。
# 用法: .\shot-app.ps1 -Name <输出文件名> [-Title <标题子串>]
param(
    [Parameter(Mandatory = $true)][string]$Name,
    [string]$Title = "SayAll"
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32Shot {
    [DllImport("user32.dll")] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$outDir = Join-Path (Get-Location) "artifacts\ui-selftest"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$found = [IntPtr]::Zero
$deadline = (Get-Date).AddSeconds(20)
while ((Get-Date) -lt $deadline -and $found -eq [IntPtr]::Zero) {
    Start-Sleep -Milliseconds 500
    $procs = Get-Process | Where-Object { $_.MainWindowTitle -like "*$Title*" -and $_.MainWindowHandle -ne 0 }
    if ($procs) { $found = $procs[0].MainWindowHandle }
}
if ($found -eq [IntPtr]::Zero) { throw "未找到标题包含 $Title 的窗口" }

[Win32Shot]::SetForegroundWindow($found) | Out-Null
Start-Sleep -Milliseconds 600
$rect = New-Object Win32Shot+RECT
[Win32Shot]::GetWindowRect($found, [ref]$rect) | Out-Null
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
$bitmap = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
$path = Join-Path $outDir "$Name.png"
$bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose(); $bitmap.Dispose()
Write-Host "saved: $path ($width x $height)"
