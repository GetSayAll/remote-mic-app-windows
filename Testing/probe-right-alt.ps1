$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class KeyProbe {
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    [StructLayout(LayoutKind.Sequential)]
    struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    [StructLayout(LayoutKind.Explicit)]
    struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    const uint KEYEVENTF_EXTENDEDKEY = 0x1;
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint KEYEVENTF_SCANCODE = 0x8;
    const uint INPUT_KEYBOARD = 1;
    static INPUT Scan(ushort scan, uint flags) {
        INPUT i = new INPUT();
        i.type = INPUT_KEYBOARD;
        i.u.ki.wVk = 0;
        i.u.ki.wScan = scan;
        i.u.ki.dwFlags = flags;
        return i;
    }
    static INPUT Vk(ushort vk, uint flags) {
        INPUT i = new INPUT();
        i.type = INPUT_KEYBOARD;
        i.u.ki.wVk = vk;
        i.u.ki.wScan = 0;
        i.u.ki.dwFlags = flags;
        return i;
    }
    public static uint RightAltDown() {
        return SendInput(1, new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint RightAltUp() {
        return SendInput(1, new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint VkDown(ushort vk) {
        return SendInput(1, new INPUT[] { Vk(vk, 0) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint VkUp(ushort vk) {
        return SendInput(1, new INPUT[] { Vk(vk, KEYEVENTF_KEYUP) }, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@

function Get-RmenuDown { [bool](([KeyProbe]::GetAsyncKeyState(0xA5)) -band 0x8000) }

$log = New-Object System.Collections.Generic.List[string]
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$injected = $false
$prev = $false
while ($sw.Elapsed.TotalSeconds -lt 3.0) {
    $cur = Get-RmenuDown
    if ($cur -ne $prev) {
        $log += ("{0:N3}s {1}" -f $sw.Elapsed.TotalSeconds, $(if ($cur) { 'DOWN' } else { 'UP' }))
        $prev = $cur
    }
    if (-not $injected -and $sw.Elapsed.TotalSeconds -ge 1.0) {
        $sent = [KeyProbe]::RightAltDown()
        $log += ("{0:N3}s inject-down sent={1}" -f $sw.Elapsed.TotalSeconds, $sent)
        $injected = $true
    }
    if ($injected -and $sw.Elapsed.TotalSeconds -ge 1.8 -and -not $script:upDone) {
        $sent = [KeyProbe]::RightAltUp()
        $log += ("{0:N3}s inject-up sent={1}" -f $sw.Elapsed.TotalSeconds, $sent)
        $script:upDone = $true
    }
    Start-Sleep -Milliseconds 5
}
$log | ForEach-Object { Write-Host $_ }
