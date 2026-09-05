# 把 SayAll 窗口移到屏幕 (0,0) 并置顶（截图防遮挡用）。
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinMove {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
"@
$p = Get-Process sayall-windows-app -ErrorAction Stop | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
# HWND_TOPMOST = -1；SWP_NOSIZE = 0x0001 只移动不改尺寸。
[WinMove]::SetWindowPos($p.MainWindowHandle, [IntPtr](-1), 0, 0, 0, 0, 0x0001) | Out-Null
[WinMove]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Write-Host "moved to (0,0), topmost"
