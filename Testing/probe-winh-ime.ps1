$ErrorActionPreference = 'Continue'
$wd = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ts = Get-Date -Format 'HHmmss'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Mx7 {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder s, int n);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hWnd, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    static INPUT Mk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static uint Down(ushort vk) { return SendInput(1, new INPUT[] { Mk(vk, 0) }, Marshal.SizeOf(typeof(INPUT))); }
    public static uint Up(ushort vk) { return SendInput(1, new INPUT[] { Mk(vk, 2) }, Marshal.SizeOf(typeof(INPUT))); }
    public static uint Tap(ushort vk) { return SendInput(2, new INPUT[] { Mk(vk, 0), Mk(vk, 2) }, Marshal.SizeOf(typeof(INPUT))); }
    public static string FgTitle() { var sb = new StringBuilder(256); GetWindowText(GetForegroundWindow(), sb, 256); return sb.ToString(); }
    public static string DoubaoWinStates() {
        var sb = new StringBuilder();
        EnumWindows(delegate(IntPtr h, IntPtr lp) {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == 9772) {
                var cn = new StringBuilder(128); GetClassName(h, cn, 128);
                string c = cn.ToString();
                if (c.StartsWith("Oime") || c.StartsWith("Ome")) sb.Append(c + ":vis=" + IsWindowVisible(h) + " ");
            }
            return true;
        }, IntPtr.Zero);
        return sb.ToString();
    }
}
'@
function Log([string]$msg) { Write-Host ("[" + (Get-Date -Format 'HH:mm:ss.fff') + "] " + $msg) }
function Save-Shot([string]$path) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

Log ("mx5 start ts=$ts fg='" + [Mx7]::FgTitle() + "' doubao=" + [Mx7]::DoubaoWinStates())
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object StartTime -Descending | Select-Object -First 1
if ($np) { [Mx7]::SetForegroundWindow($np.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 700 }
Log ("fg='" + [Mx7]::FgTitle() + "' doubao=" + [Mx7]::DoubaoWinStates())

function WinHPer([string]$tag) {
    Log ("$tag : per-event Win+H, doubao-before=" + [Mx7]::DoubaoWinStates())
    Save-Shot "$wd\Testing\mx5-$tag-before.png"
    $r1 = [Mx7]::Down(0x5B); Start-Sleep -Milliseconds 80
    $r2 = [Mx7]::Down(0x48); Start-Sleep -Milliseconds 60
    $r3 = [Mx7]::Up(0x48); Start-Sleep -Milliseconds 60
    $r4 = [Mx7]::Up(0x5B)
    Log ("$tag : sent LWinD=$r1 HD=$r2 HU=$r3 LWinU=$r4")
    Start-Sleep -Milliseconds 1500
    Save-Shot "$wd\Testing\mx5-$tag-after.png"
    Start-Sleep -Milliseconds 500
    Save-Shot "$wd\Testing\mx5-$tag-after2.png"
    [void][Mx7]::Tap(0x1B)
    Start-Sleep -Milliseconds 600
}

# attempt 1: current slot (expect Doubao active)
WinHPer '01-doubao'

# advance slot, retest
Log "advance slot"
[void][Mx7]::Down(0x5B); Start-Sleep -Milliseconds 200; [void][Mx7]::Tap(0x20); Start-Sleep -Milliseconds 350; [void][Mx7]::Up(0x5B)
Start-Sleep -Milliseconds 1000
Log ("after advance: doubao=" + [Mx7]::DoubaoWinStates())
WinHPer '02-next'

# advance again, retest
Log "advance slot again"
[void][Mx7]::Down(0x5B); Start-Sleep -Milliseconds 200; [void][Mx7]::Tap(0x20); Start-Sleep -Milliseconds 350; [void][Mx7]::Up(0x5B)
Start-Sleep -Milliseconds 1000
Log ("after advance2: doubao=" + [Mx7]::DoubaoWinStates())
WinHPer '03-next2'

Log ("final doubao=" + [Mx7]::DoubaoWinStates())
Log "mx5 done"
