$ErrorActionPreference = 'Continue'
$wd = "C:\Users\Administrator\Documents\Codex\remote-mic-app-windows"
$ts = Get-Date -Format 'HHmmss'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Fin {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    const uint KEYUP = 0x2; const uint EXT = 0x1; const uint SCAN = 0x8; const uint UNICODE = 0x4;
    static INPUT Mk(ushort vk, ushort scan, uint flags, ulong extra) {
        INPUT i = new INPUT(); i.type = 1;
        i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; i.u.ki.dwExtraInfo = (IntPtr)extra;
        return i;
    }
    public static uint ATapExtra(ulong extra) {
        INPUT[] a = new INPUT[] { Mk(0x41, 0, 0, extra), Mk(0x41, 0, KEYUP, extra) };
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint UniTapZ() {
        INPUT[] a = new INPUT[] { Mk(0, (ushort)'z', UNICODE, 0), Mk(0, (ushort)'z', UNICODE | KEYUP, 0) };
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void KeybdEventRAlt() {
        keybd_event(0, 0x38, SCAN | EXT, UIntPtr.Zero);
        keybd_event(0, 0x38, SCAN | EXT | KEYUP, UIntPtr.Zero);
    }
    // raw device enumeration
    [StructLayout(LayoutKind.Sequential)]
    public struct RAWINPUTDEVICELIST { public IntPtr hDevice; public uint dwType; public IntPtr dummy; }
    [DllImport("user32.dll")] public static extern uint GetRawInputDeviceList(IntPtr pRawInputDeviceList, ref uint uiNumDevices, uint cbSize);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern uint GetRawInputDeviceInfo(IntPtr hDevice, uint uiCommand, System.Text.StringBuilder pData, ref uint pcbSize);
    public static string RawDevices() {
        uint n = 0;
        GetRawInputDeviceList(IntPtr.Zero, ref n, (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICELIST)));
        IntPtr buf = Marshal.AllocHGlobal((int)(n * Marshal.SizeOf(typeof(RAWINPUTDEVICELIST))));
        GetRawInputDeviceList(buf, ref n, (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICELIST)));
        var sb = new StringBuilder();
        for (uint i = 0; i < n; i++) {
            RAWINPUTDEVICELIST d = (RAWINPUTDEVICELIST)Marshal.PtrToStructure(new IntPtr(buf.ToInt64() + i * Marshal.SizeOf(typeof(RAWINPUTDEVICELIST))), typeof(RAWINPUTDEVICELIST));
            var name = new StringBuilder(512);
            uint sz = 512;
            GetRawInputDeviceInfo(d.hDevice, 0x20000007 /*RIDI_DEVICENAME*/, name, ref sz); // may fail, try STRING later
            var desc = new StringBuilder(512);
            uint sz2 = 512;
            GetRawInputDeviceInfo(d.hDevice, 0x20000005 /*RIDI_PRODUCT... actually 0x20000005 is RIDI_DEVICEPRODUCT? not std; use preparseD fail-safe*/, desc, ref sz2);
            sb.AppendLine("dev 0x" + d.hDevice.ToInt64().ToString("X") + " type=" + d.dwType + " name=" + name);
        }
        Marshal.FreeHGlobal(buf);
        return sb.ToString();
    }
}
'@
Write-Host ("[" + (Get-Date -Format 'HH:mm:ss.fff') + "] final probe start")
$llmonLog = "$wd\Testing\llmon-final-$ts.log"
$llProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',$llmonLog,'-Seconds','40' -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 2
$r1 = [Fin]::ATapExtra([uint64]0x1122334455)
Write-Host ("A-tap extra sent=$r1")
Start-Sleep -Milliseconds 600
$r2 = [Fin]::UniTapZ()
Write-Host ("UNICODE z sent=$r2")
Start-Sleep -Milliseconds 600
[Fin]::KeybdEventRAlt()
Write-Host "keybd_event RAlt done"
Start-Sleep -Milliseconds 600
try {
    [void][Windows.UI.Input.Preview.Injection.InputInjector, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    $inj = [Windows.UI.Input.Preview.Injection.InputInjector]::TryCreate()
    $info = New-Object 'Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo[]' 1
    $d = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new(); $d.VirtualKey = [uint16]70; $info[0] = $d
    $inj.InjectKeyboardInput($info)
    Start-Sleep -Milliseconds 200
    $u = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new(); $u.VirtualKey = [uint16]70; $u.KeyOptions = [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::KeyUp
    $info[0] = $u; $inj.InjectKeyboardInput($info)
    Write-Host "InputInjector F tap done"
} catch { Write-Host ("ii error: " + $_.Exception.Message) }
Start-Sleep -Seconds 3
try { Stop-Process -Id $llProc.Id -Force -ErrorAction SilentlyContinue } catch { }
Write-Host '--- raw devices ---'
Write-Host ([Fin]::RawDevices())
Write-Host '--- llmon final log ---'
Get-Content $llmonLog -ErrorAction SilentlyContinue
