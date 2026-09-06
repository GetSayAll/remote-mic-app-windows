$ErrorActionPreference = 'Stop'
$wd = "$PSScriptRoot\.."
$stamp = Get-Date -Format 'HHmmss'
$logPath = "$wd\Testing\hookmon-$stamp.log"

Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\probe-hookmon.ps1","$logPath",'20' -WindowStyle Hidden
Start-Sleep -Seconds 4

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp4 {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint KEYEVENTF_EXTENDEDKEY = 0x1;
    const uint KEYEVENTF_SCANCODE = 0x8;
    const uint INPUT_KEYBOARD = 1;
    public static INPUT Vk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static INPUT Scan(ushort scan, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; return i; }
    public static uint Batch(INPUT[] list) { return SendInput((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT))); }
    public static void RightAltDown() { Batch(new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY) }); }
    public static void RightAltUp() { Batch(new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP) }); }
    public static bool RmenuDown { get { return (GetAsyncKeyState(0xA5) & 0x8000) != 0; } }
}
'@

function Save-Shot([string]$path) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# 0. sanity: monitor must see an F12 tap
[Exp4]::Batch(@([Exp4]::Vk(0x7B, 0), [Exp4]::Vk(0x7B, 2))) | Out-Null
Start-Sleep -Milliseconds 700

# 1. focus notepad, hold Right Alt 1.5s, screenshot during hold
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) { [Exp4]::SetForegroundWindow($np.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800 }
[Exp4]::RightAltDown()
Start-Sleep -Milliseconds 400
$early = [Exp4]::RmenuDown
Start-Sleep -Milliseconds 1100
$late = [Exp4]::RmenuDown
Save-Shot "$wd\Testing\r3-during-hold.png"
[Exp4]::RightAltUp()
Start-Sleep -Milliseconds 1500
Write-Host "r3 early-down=$early late-down=$late"

Start-Sleep -Seconds 17
Write-Host '--- HOOK LOG ---'
Get-Content $logPath
