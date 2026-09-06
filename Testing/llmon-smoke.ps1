$ErrorActionPreference = 'Continue'
$wd = "C:\Users\Administrator\Documents\Codex\remote-mic-app-windows"
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Inj40 {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    static INPUT Mk(ushort vk, ushort scan, uint flags, ulong extra) {
        INPUT i = new INPUT(); i.type = 1;
        i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags;
        i.u.ki.dwExtraInfo = (IntPtr)extra;
        return i;
    }
    public static uint Tap(ushort vk, uint flags, ulong extra) {
        INPUT[] a = new INPUT[] { Mk(vk, 0, flags, extra), Mk(vk, 0, flags | 2, extra) };
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Down(ushort vk, ushort scan, uint flags, ulong extra) {
        INPUT[] a = new INPUT[] { Mk(vk, scan, flags, extra) };
        return SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Up(ushort vk, ushort scan, uint flags, ulong extra) {
        INPUT[] a = new INPUT[] { Mk(vk, scan, flags | 2, extra) };
        return SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@
Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',"$wd\Testing\llmon-test.log",'-Seconds','12' -WindowStyle Hidden
Start-Sleep -Seconds 3
$r1 = [Inj40]::Tap(0x7C, 0, 0)          # F13 tap
$r2 = [Inj40]::Down(0xA5, 0x38, 0x9, 0) # RAlt scan+ext down (KEYEVENTF_SCANCODE|EXTENDEDKEY=0x9)
Start-Sleep -Milliseconds 400
$r3 = [Inj40]::Up(0xA5, 0x38, 0xB, 0)
$r4 = [Inj40]::Tap(0x41, 0, 0)          # 'A' tap wVk
$r5 = [Inj40]::Tap(0x41, 0, 0, 0x1234)  # 'A' tap with dwExtraInfo marker
Write-Host ("sent: F13={0} RAltDown={1} RAltUp={2} Atap={3} AtapExtra={4}" -f $r1,$r2,$r3,$r4,$r5)
Start-Sleep -Seconds 3
Write-Host '--- llmon-test.log ---'
Get-Content "$wd\Testing\llmon-test.log"
