param(
    [string]$LogPath = "$PSScriptRoot\llmon.log",
    [int]$Seconds = 180
)
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies @('System.Windows.Forms.dll','System.dll','System.Core.dll') -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.IO;
using System.Text;

public static class LLMon {
    [StructLayout(LayoutKind.Sequential)]
    public struct KBDLLHOOKSTRUCT { public uint vkCode; public uint scanCode; public uint flags; public uint time; public UIntPtr dwExtraInfo; }
    public delegate IntPtr HookProc(int nCode, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowsHookEx(int idHook, HookProc lpfn, IntPtr hMod, uint dwThreadId);
    [DllImport("user32.dll")] public static extern bool UnhookWindowsHookEx(IntPtr hhk);
    [DllImport("user32.dll")] public static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll")] public static extern IntPtr GetModuleHandle(string lpModuleName);

    // raw input
    [StructLayout(LayoutKind.Sequential)]
    public struct RAWINPUTDEVICE { public ushort usUsagePage; public ushort usUsage; public uint dwFlags; public IntPtr hwndTarget; }
    [DllImport("user32.dll")] public static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] pRawInputDevices, uint uiNumDevices, uint cbSize);
    [DllImport("user32.dll")] public static extern uint GetRawInputData(IntPtr hRawInput, uint uiCommand, IntPtr pData, ref uint pcbSize, uint cbSizeHeader);

    public static StreamWriter Writer;
    public static HookProc ProcRef; // keep delegate alive
    public static IntPtr Hook = IntPtr.Zero;
    public static long Count = 0;
    static readonly object _lock = new object();

    public static void Init(string path) {
        Writer = new StreamWriter(path, false, Encoding.ASCII) { AutoFlush = true };
        Writer.WriteLine("llmon start " + DateTime.Now.ToString("HH:mm:ss.fff") + " pid=" + System.Diagnostics.Process.GetCurrentProcess().Id);
    }
    public static void Stop() {
        try { if (Hook != IntPtr.Zero) { UnhookWindowsHookEx(Hook); Hook = IntPtr.Zero; } } catch { }
        try { if (Writer != null) { Writer.WriteLine("llmon stop " + DateTime.Now.ToString("HH:mm:ss.fff") + " events=" + Count); Writer.Close(); } } catch { }
    }
    public static void InstallHook() {
        ProcRef = new HookProc(HookCb);
        Hook = SetWindowsHookEx(13, ProcRef, GetModuleHandle(null), 0);
        Writer.WriteLine("hook installed handle=0x" + Hook.ToString("X") + " err=" + Marshal.GetLastWin32Error());
    }
    static IntPtr HookCb(int nCode, IntPtr wParam, IntPtr lParam) {
        if (nCode >= 0) {
            try {
                KBDLLHOOKSTRUCT k = (KBDLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(KBDLLHOOKSTRUCT));
                lock (_lock) {
                    Count++;
                    Writer.WriteLine("LL " + DateTime.Now.ToString("HH:mm:ss.fff")
                        + " wparam=0x" + wParam.ToString("X")
                        + " vk=0x" + k.vkCode.ToString("X4")
                        + " scan=0x" + k.scanCode.ToString("X4")
                        + " flags=0x" + k.flags.ToString("X4")
                        + " time=" + k.time
                        + " extra=0x" + k.dwExtraInfo.ToUInt64().ToString("X"));
                }
            } catch { }
        }
        return CallNextHookEx(Hook, nCode, wParam, lParam);
    }
}

public class RawWin : System.Windows.Forms.NativeWindow {
    [StructLayout(LayoutKind.Sequential)]
    public struct RAWINPUTHEADER { public uint dwType; public uint dwSize; public IntPtr hDevice; public IntPtr wParam; }
    [StructLayout(LayoutKind.Sequential)]
    public struct RAWKEYBOARD { public ushort MakeCode; public ushort Flags; public ushort Reserved; public ushort VKey; public uint Message; public uint ExtraInformation; }
    const int WM_INPUT = 0x00FF;
    const uint RIDEV_INPUTSINK = 0x00000100;
    public void Create() {
        System.Windows.Forms.CreateParams cp = new System.Windows.Forms.CreateParams();
        cp.Caption = "llmon_raw";
        CreateHandle(cp);
        LLMon.RAWINPUTDEVICE[] rid = new LLMon.RAWINPUTDEVICE[1];
        rid[0].usUsagePage = 1; rid[0].usUsage = 6; rid[0].dwFlags = RIDEV_INPUTSINK; rid[0].hwndTarget = this.Handle;
        bool ok = LLMon.RegisterRawInputDevices(rid, 1, (uint)Marshal.SizeOf(typeof(LLMon.RAWINPUTDEVICE)));
        LLMon.Writer.WriteLine("rawinput registered=" + ok + " hwnd=0x" + Handle.ToString("X") + " err=" + Marshal.GetLastWin32Error());
    }
    protected override void WndProc(ref System.Windows.Forms.Message m) {
        if (m.Msg == WM_INPUT) {
            try {
                uint size = 0;
                LLMon.GetRawInputData(m.LParam, 0x10000003, IntPtr.Zero, ref size, (uint)Marshal.SizeOf(typeof(RAWINPUTHEADER)));
                IntPtr buf = Marshal.AllocHGlobal((int)size);
                LLMon.GetRawInputData(m.LParam, 0x10000003, buf, ref size, (uint)Marshal.SizeOf(typeof(RAWINPUTHEADER)));
                byte[] bytes = new byte[size];
                Marshal.Copy(buf, bytes, 0, (int)size);
                Marshal.FreeHGlobal(buf);
                // parse manually: x64 header = 24 bytes (dwType:4, dwSize:4, hDevice:8, wParam:8), then RAWKEYBOARD
                if (size >= 40) {
                    uint dwType = BitConverter.ToUInt32(bytes, 0);
                    IntPtr hDevice = (IntPtr)BitConverter.ToInt64(bytes, 8);
                    ushort make = BitConverter.ToUInt16(bytes, 24);
                    ushort kflags = BitConverter.ToUInt16(bytes, 26);
                    ushort vkey = BitConverter.ToUInt16(bytes, 30);
                    uint msg = BitConverter.ToUInt32(bytes, 32);
                    lock (typeof(LLMon)) {
                        LLMon.Writer.WriteLine("RAW " + DateTime.Now.ToString("HH:mm:ss.fff")
                            + " type=" + dwType
                            + " msg=0x" + msg.ToString("X")
                            + " vk=0x" + vkey.ToString("X4")
                            + " make=0x" + make.ToString("X4")
                            + " flags=0x" + kflags.ToString("X4")
                            + " dev=0x" + hDevice.ToInt64().ToString("X"));
                    }
                }
            } catch { }
        }
        base.WndProc(ref m);
    }
}
'@
[LLMon]::Init($LogPath)
[LLMon]::InstallHook()
$raw = New-Object RawWin
$raw.Create()

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = ($Seconds * 1000)
$timer.Add_Tick({ $timer.Stop(); [LLMon]::Stop(); [System.Windows.Forms.Application]::ExitThread() })
$timer.Start()
[System.Windows.Forms.Application]::Run()
Write-Host ("llmon exited, events=" + [LLMon]::Count)
