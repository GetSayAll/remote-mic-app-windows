$ErrorActionPreference = 'Stop'
$wd = "$PSScriptRoot\.."
Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\probe-hookmon.ps1",'30' -WindowStyle Hidden
Start-Sleep -Seconds 3

Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp3 {
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

function Focus-Proc([string]$name) {
    $p = Get-Process $name -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ($p) { [Exp3]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800 }
    return $p
}
function Hold-Test([string]$tag) {
    $t0 = Get-Date
    [Exp3]::RightAltDown()
    Start-Sleep -Milliseconds 400
    $early = [Exp3]::RmenuDown
    Start-Sleep -Milliseconds 1100
    $late = [Exp3]::RmenuDown
    [Exp3]::RightAltUp()
    Start-Sleep -Milliseconds 600
    Write-Host "$tag early-down=$early late-down=$late"
}

Write-Host "=== A: notepad direct hold ==="
Focus-Proc "notepad" | Out-Null
Hold-Test "A-notepad"

Write-Host "=== B: notepad typed first ==="
Focus-Proc "notepad" | Out-Null
[Exp3]::Batch(@([Exp3]::Vk(0x4E, 0), [Exp3]::Vk(0x4E, 2))) | Out-Null
Start-Sleep -Milliseconds 700
Hold-Test "B-notepad-typed"

Write-Host "=== C: doubao settings focused ==="
Focus-Proc "DoubaoImeSettings" | Out-Null
Hold-Test "C-doubao-settings"

Write-Host "=== waiting for monitor ==="
Start-Sleep -Seconds 18
Get-Content "$wd\Testing\hookmon.log"
