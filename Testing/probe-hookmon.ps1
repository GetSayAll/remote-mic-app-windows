$ErrorActionPreference = 'Stop'
Add-Type -ReferencedAssemblies System.Windows.Forms @'
using System;
using System.Runtime.InteropServices;
using System.IO;
public static class HookMon {
    public delegate IntPtr HookProc(int nCode, IntPtr wParam, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct KBDLLHOOKSTRUCT { public uint vkCode; public uint scanCode; public uint flags; public uint time; public UIntPtr dwExtraInfo; }
    [DllImport("user32.dll")] public static extern IntPtr SetWindowsHookEx(int idHook, HookProc lpfn, IntPtr hMod, uint dwThreadId);
    [DllImport("user32.dll")] public static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll")] public static extern IntPtr GetModuleHandle(string name);
    [DllImport("kernel32.dll")] public static extern short GetAsyncKeyState(int vKey);
    public static HookProc proc;
    public static IntPtr hook;
    public static StreamWriter log;
    public static IntPtr Callback(int nCode, IntPtr wParam, IntPtr lParam) {
        if (nCode >= 0) {
            KBDLLHOOKSTRUCT s = (KBDLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(KBDLLHOOKSTRUCT));
            string msg = (wParam.ToInt64() == 0x0100 || wParam.ToInt64() == 0x0104) ? "DOWN" : "UP";
            log.WriteLine(string.Format("{0} vk=0x{1:X2} scan=0x{2:X2} flags=0x{3:X} injected={4} extended={5}",
                msg, s.vkCode, s.scanCode, s.flags, (s.flags & 0x10) != 0, (s.flags & 0x1) != 0));
        }
        return CallNextHookEx(hook, nCode, wParam, lParam);
    }
    public static void Run(string path, int seconds) {
        log = new StreamWriter(path, false);
        log.AutoFlush = true;
        proc = Callback;
        hook = SetWindowsHookEx(13, proc, GetModuleHandle(null), 0);
        log.WriteLine("hook-installed=" + (hook != IntPtr.Zero));
        var sw = System.Diagnostics.Stopwatch.StartNew();
        bool lastDown = false;
        while (sw.Elapsed.TotalSeconds < seconds) {
            bool down = (GetAsyncKeyState(0xA5) & 0x8000) != 0;
            if (down != lastDown) { log.WriteLine(string.Format("POLL {0:N3}s rmenu={1}", sw.Elapsed.TotalSeconds, down)); lastDown = down; }
            System.Threading.Thread.Sleep(5);
            System.Windows.Forms.Application.DoEvents();
        }
        log.WriteLine("done");
        log.Dispose();
    }
}
'@
Add-Type -AssemblyName System.Windows.Forms
$monPath = if ($args.Count -ge 2) { $args[0] } else { "Testing\hookmon.log" }
$monSec = if ($args.Count -ge 2) { [int]$args[1] } else { [int]$args[0] }
[HookMon]::Run($monPath, $monSec)
