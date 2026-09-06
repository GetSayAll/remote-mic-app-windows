param(
    [int]$DownDelayMs = 300,
    [int]$HoldMs = 1400
)
$ErrorActionPreference = 'Continue'
Add-Type @'
using System;
using System.Threading;
using System.Runtime.InteropServices;
public static class JPlay {
    [StructLayout(LayoutKind.Sequential)]
    public struct EVENTMSG { public uint message; public uint paramL; public uint paramH; public uint time; public IntPtr hwnd; }
    public delegate IntPtr HookProc(int nCode, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowsHookEx(int idHook, HookProc lpfn, IntPtr hMod, uint dwThreadId);
    [DllImport("user32.dll")] public static extern bool UnhookWindowsHookEx(IntPtr hhk);
    [DllImport("user32.dll")] public static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll")] public static extern IntPtr GetModuleHandle(string lpModuleName);
    [StructLayout(LayoutKind.Sequential)]
    public struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; uint time; int px; int py; }
    [DllImport("user32.dll")] public static extern int GetMessage(out MSG msg, IntPtr hWnd, uint min, uint max);
    [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint idThread, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);

    public static IntPtr Hook = IntPtr.Zero;
    static HookProc ProcRef;
    static EVENTMSG[] Events;
    static int Index = 0;
    static uint MainThreadId;

    public static void Play(int downDelay, int hold) {
        uint t0 = (uint)Environment.TickCount + (uint)downDelay;
        Events = new EVENTMSG[2];
        Events[0].message = 0x0104; // WM_SYSKEYDOWN
        Events[0].paramL = 0xA5;    // VK_RMENU
        Events[0].paramH = 0x21380001; // repeat=1 scan=0x38 ext+context
        Events[0].time = t0;
        Events[1].message = 0x0105; // WM_SYSKEYUP
        Events[1].paramL = 0xA5;
        Events[1].paramH = 0xE1380001;
        Events[1].time = t0 + (uint)hold;
        Index = 0;
        ProcRef = new HookProc(Cb);
        MainThreadId = GetCurrentThreadId();
        Hook = SetWindowsHookEx(1, ProcRef, GetModuleHandle(null), 0); // WH_JOURNALPLAYBACK
        Console.WriteLine("hook=0x" + Hook.ToString("X") + " err=" + Marshal.GetLastWin32Error());
        if (Hook == IntPtr.Zero) return;
        // auto unhook via timer message
        Timer t = new Timer(_ => {
            PostThreadMessage(MainThreadId, 0x0012 /*WM_QUIT*/, IntPtr.Zero, IntPtr.Zero);
        }, null, downDelay + hold + 900, Timeout.Infinite);
        MSG msg;
        while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) { }
        UnhookWindowsHookEx(Hook);
        Console.WriteLine("unhooked, ralt=" + ((GetAsyncKeyState(0xA5) & 0x8000) != 0));
    }
    static IntPtr Cb(int nCode, IntPtr wParam, IntPtr lParam) {
        if (nCode == 1) { // HC_GETNEXT
            if (Index < Events.Length) {
                Marshal.StructureToPtr(Events[Index], lParam, false);
                int delay = (int)Events[Index].time - Environment.TickCount;
                return (IntPtr)(delay > 0 ? delay : 0);
            }
            return (IntPtr)60000; // hold stream; we unhook soon
        } else if (nCode == 2) { // HC_SKIP
            Index++;
            return IntPtr.Zero;
        }
        return IntPtr.Zero;
    }
}
'@
[JPlay]::Play($DownDelayMs, $HoldMs)
Write-Host "jplay done"
