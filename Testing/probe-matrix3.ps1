$ErrorActionPreference = 'Continue'
$wd = "C:\Users\Administrator\Documents\Codex\remote-mic-app-windows"
$ts = Get-Date -Format 'HHmmss'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Mx3 {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("kernel32.dll")] public static extern uint GetLastError();
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hWnd, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
    static INPUT Mk(ushort vk, ushort scan, uint flags) {
        INPUT i = new INPUT(); i.type = 1;
        i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags;
        return i;
    }
    public static uint Tap(ushort vk) {
        INPUT[] a = new INPUT[] { Mk(vk, 0, 0), Mk(vk, 0, 2) };
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Down(ushort vk, ushort scan, uint flags) {
        return SendInput(1, new INPUT[] { Mk(vk, scan, flags) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Up(ushort vk, ushort scan, uint flags) {
        return SendInput(1, new INPUT[] { Mk(vk, scan, flags | 2) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint WinH() {
        INPUT[] a = new INPUT[] { Mk(0x5B, 0, 0), Mk(0x48, 0, 0), Mk(0x48, 0, 2), Mk(0x5B, 0, 2) };
        return SendInput(4, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Tap0(ushort vk) { // marker: vk with extra marker scan
        INPUT[] a = new INPUT[] { Mk(vk, 0xE1E1, 0), Mk(vk, 0xE1E1, 2) };
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static string FgTitle() {
        var sb = new StringBuilder(256);
        GetWindowText(GetForegroundWindow(), sb, 256);
        return sb.ToString();
    }
    public static bool RAltDown { get { return (GetAsyncKeyState(0xA5) & 0x8000) != 0; } }
    public static string ModifierStates() {
        int[] vks = new int[] { 0x5B, 0x5C, 0x10, 0xA0, 0xA1, 0x11, 0xA2, 0xA3, 0x12, 0xA4, 0xA5 };
        string[] names = new string[] { "LWin", "RWin", "Shift", "LShift", "RShift", "Ctrl", "LCtrl", "RCtrl", "Alt", "LAlt", "RAlt" };
        var sb = new StringBuilder();
        for (int i = 0; i < vks.Length; i++) {
            if ((GetAsyncKeyState(vks[i]) & 0x8000) != 0) { sb.Append(names[i]); sb.Append("=DOWN "); }
        }
        return sb.Length == 0 ? "(none)" : sb.ToString();
    }
    public static string WindowsOfPid(uint pid) {
        var sb = new StringBuilder();
        EnumWindows(delegate(IntPtr h, IntPtr lp) {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid) {
                var cn = new StringBuilder(128); GetClassName(h, cn, 128);
                var tt = new StringBuilder(128); GetWindowText(h, tt, 128);
                sb.Append("[hwnd=0x" + h.ToString("X") + " class=" + cn + " title=" + tt + " vis=" + IsWindowVisible(h) + "] ");
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
function Diff-Count([string]$a, [string]$b) {
    $imgA = [System.Drawing.Bitmap]::FromFile($a); $imgB = [System.Drawing.Bitmap]::FromFile($b)
    $w = [Math]::Min($imgA.Width, $imgB.Width); $h = [Math]::Min($imgA.Height, $imgB.Height)
    $diff = 0
    for ($y = 0; $y -lt $h; $y += 3) { for ($x = 0; $x -lt $w; $x += 3) {
        $pa = $imgA.GetPixel($x, $y); $pb = $imgB.GetPixel($x, $y)
        if ([Math]::Abs($pa.R - $pb.R) + [Math]::Abs($pa.G - $pb.G) + [Math]::Abs($pa.B - $pb.B) -gt 30) { $diff++ }
    } }
    $imgA.Dispose(); $imgB.Dispose(); return $diff
}
function Get-ImePids() {
    $map = @{}
    foreach ($n in @('ImeService','ChsIME','ImeWatchdog','DoubaoImeSettings')) {
        $p = Get-Process $n -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($p) { $map[$n] = $p.Id }
    }
    return $map
}
function Dump-ImeWindows([string]$tag) {
    $map = Get-ImePids
    foreach ($k in $map.Keys) {
        $w = [Mx3]::WindowsOfPid([uint32]$map[$k])
        if ($w) { Log ("IMEWIN $tag ${k}(pid=" + $map[$k] + "): " + $w) }
    }
}
function Marker([uint16]$vk) { $r = [Mx3]::Tap0($vk); Log ("marker vk=0x" + $vk.ToString('X') + " sent=" + $r) }

Log ("run3 start ts=$ts mods=" + [Mx3]::ModifierStates())

# ---- A: repair InputMethodOverride registry (corrupted earlier) ----
try {
    Remove-ItemProperty -Path 'HKCU:\Control Panel\International\User Profile' -Name 'InputMethodOverride' -ErrorAction Stop
    Log "A: InputMethodOverride removed (was corrupted string)"
} catch { Log ("A: remove failed: " + $_.Exception.Message) }
try { $o = Get-WinDefaultInputMethodOverride; Log ("A: override now: " + $(if ($null -eq $o -or $o.InputTip -eq $null) { '(empty)' } else { $o.InputTip })) } catch { Log ("A: override get: " + $_.Exception.Message) }

# ---- B: notepad ----
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object StartTime -Descending | Select-Object -First 1
if (-not $np) { $npProc = Start-Process notepad -PassThru; Start-Sleep -Seconds 2; $np = Get-Process -Id $npProc.Id }
$npHwnd = $np.MainWindowHandle
[Mx3]::SetWindowPos($npHwnd, [IntPtr]::Zero, 60, 60, 900, 600, 0x0040) | Out-Null
Start-Sleep -Milliseconds 400
[Mx3]::SetForegroundWindow($npHwnd) | Out-Null
Start-Sleep -Milliseconds 500
$r = New-Object Mx3+RECT
[Mx3]::GetWindowRect($npHwnd, [ref]$r) | Out-Null
$cx = [int](($r.L + $r.R) / 2); $cy = [int](($r.T + $r.B) / 2)
[Mx3]::SetCursorPos($cx, $cy) | Out-Null
Start-Sleep -Milliseconds 150
[Mx3]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
[Mx3]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 900
Log ("B: notepad pid=" + $np.Id + " fg='" + [Mx3]::FgTitle() + "'")
Save-Shot "$wd\Testing\mx3-00-baseline.png"
Dump-ImeWindows 'baseline'

# ---- C: Win+Space flyout probe ----
Log "C: hold LWin + tap Space (flyout probe)"
[void][Mx3]::Down(0x5B, 0, 0)
Start-Sleep -Milliseconds 200
[void][Mx3]::Tap(0x20)
Start-Sleep -Milliseconds 600
Save-Shot "$wd\Testing\mx3-01-flyout.png"
[void][Mx3]::Up(0x5B, 0, 2)
Start-Sleep -Milliseconds 800
Save-Shot "$wd\Testing\mx3-02-after-flyout.png"
Dump-ImeWindows 'after-flyout'

# ---- D: slot cycle loop ----
$slots = 4
for ($i = 1; $i -le $slots; $i++) {
    if ($i -gt 1) {
        Log ("D: advance slot (tap Space under held LWin) iter=$i")
        [void][Mx3]::Down(0x5B, 0, 0)
        Start-Sleep -Milliseconds 200
        [void][Mx3]::Tap(0x20)
        Start-Sleep -Milliseconds 350
        [void][Mx3]::Up(0x5B, 0, 2)
        Start-Sleep -Milliseconds 900
    }
    Log ("D: slot iter=$i start; mods=" + [Mx3]::ModifierStates())
    Save-Shot "$wd\Testing\mx3-1$i-idle.png"
    Dump-ImeWindows ("slot$i-idle")
    # composition probe
    Marker 0xE7
    [void][Mx3]::Tap(0x41)
    Start-Sleep -Milliseconds 600
    Save-Shot "$wd\Testing\mx3-1$i-composition.png"
    Dump-ImeWindows ("slot$i-composition")
    Log ("D: slot$i composition diff-vs-idle: " + (Diff-Count "$wd\Testing\mx3-1$i-idle.png" "$wd\Testing\mx3-1$i-composition.png"))
    [void][Mx3]::Tap(0x1B); Start-Sleep -Milliseconds 250; [void][Mx3]::Tap(0x1B)
    Start-Sleep -Milliseconds 500
    # MECH-A wVk RAlt hold
    Marker 0xE8
    try {
        $a1 = [Mx3]::Down(0xA5, 0, 0)
        Start-Sleep -Milliseconds 700
        $h1 = [Mx3]::RAltDown
        Save-Shot "$wd\Testing\mx3-1$i-wvk-hold.png"
        Start-Sleep -Milliseconds 700
        $h2 = [Mx3]::RAltDown
    } finally { $a2 = [Mx3]::Up(0xA5, 0, 2) }
    Log ("D: slot$i MECH-A wVk sentD=$a1 held1=$h1 held2=$h2 sentU=$a2")
    Log ("D: slot$i wvk-hold diff-vs-idle: " + (Diff-Count "$wd\Testing\mx3-1$i-idle.png" "$wd\Testing\mx3-1$i-wvk-hold.png"))
    Start-Sleep -Milliseconds 1200
    # MECH-B scan+ext RAlt hold
    Marker 0xE9
    try {
        $b1 = [Mx3]::Down(0, 0x38, 0x9)
        Start-Sleep -Milliseconds 700
        $g1 = [Mx3]::RAltDown
        Save-Shot "$wd\Testing\mx3-1$i-scan-hold.png"
        Start-Sleep -Milliseconds 700
        $g2 = [Mx3]::RAltDown
    } finally { $b2 = [Mx3]::Up(0, 0x38, 0xB) }
    Log ("D: slot$i MECH-B scan sentD=$b1 held1=$g1 held2=$g2 sentU=$b2")
    Log ("D: slot$i scan-hold diff-vs-idle: " + (Diff-Count "$wd\Testing\mx3-1$i-idle.png" "$wd\Testing\mx3-1$i-scan-hold.png"))
    Start-Sleep -Milliseconds 1200
    # MECH-C InputInjector hold
    Marker 0xEA
    try {
        [void][Windows.UI.Input.Preview.Injection.InputInjector, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
        [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
        [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
        $injector = [Windows.UI.Input.Preview.Injection.InputInjector]::TryCreate()
        if ($injector) {
            $info = New-Object 'Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo[]' 1
            $down = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
            $down.VirtualKey = [uint16]165; $down.ScanCode = [uint16]56
            $info[0] = $down
            $injector.InjectKeyboardInput($info)
            Start-Sleep -Milliseconds 700
            $k1 = [Mx3]::RAltDown
            Save-Shot "$wd\Testing\mx3-1$i-ii-hold.png"
            Start-Sleep -Milliseconds 700
            $k2 = [Mx3]::RAltDown
            $up = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
            $up.VirtualKey = [uint16]165; $up.ScanCode = [uint16]56
            $up.KeyOptions = [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::KeyUp -bor [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::ExtendedKey
            $info[0] = $up
            $injector.InjectKeyboardInput($info)
            Log ("D: slot$i MECH-C ii held1=$k1 held2=$k2 released")
        } else { Log "D: slot$i InputInjector null" }
    } catch { Log ("D: slot$i MECH-C error: " + $_.Exception.Message) }
    Log ("D: slot$i ii-hold diff-vs-idle: " + (Diff-Count "$wd\Testing\mx3-1$i-idle.png" "$wd\Testing\mx3-1$i-ii-hold.png"))
    Start-Sleep -Milliseconds 1000
}

# ---- E: Win+H batch-4 with correct struct ----
Log ("E: Win+H batch4 pre mods=" + [Mx3]::ModifierStates() + " fg='" + [Mx3]::FgTitle() + "'")
Save-Shot "$wd\Testing\mx3-20-winh-before.png"
$wret = [Mx3]::WinH()
$wgle = [Mx3]::GetLastError()
Log ("E: batch4 sent=$wret gle=$wgle")
Start-Sleep -Milliseconds 2500
Save-Shot "$wd\Testing\mx3-21-winh-batch.png"
Log ("E: diff: " + (Diff-Count "$wd\Testing\mx3-20-winh-before.png" "$wd\Testing\mx3-21-winh-batch.png"))
[void][Mx3]::Tap(0x1B)
Start-Sleep -Milliseconds 600

# ---- F: journal playback ----
Log "F: starting llmon for journal phase"
$llmonLog = "$wd\Testing\llmon-jp-$ts.log"
$llProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',$llmonLog,'-Seconds','90' -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 2
Log "F: starting jplay child (journal playback RAlt down/hold1.4s/up)"
$jpProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\jplay.ps1",'-DownDelayMs','400','-HoldMs','1400' -WindowStyle Hidden -PassThru -RedirectStandardOutput "$wd\Testing\jplay-out.txt" -RedirectStandardError "$wd\Testing\jplay-err.txt"
Start-Sleep -Milliseconds 1100
Log ("F: mid-hold raltAsync=" + [Mx3]::RAltDown)
Save-Shot "$wd\Testing\mx3-30-jp-hold.png"
$exited = $jpProc.WaitForExit(5000)
Log ("F: jplay exited=$exited raltAfter=" + [Mx3]::RAltDown)
Start-Sleep -Milliseconds 900
Save-Shot "$wd\Testing\mx3-31-jp-after.png"
Log ("F: jp-hold diff-vs-slot4-idle: " + (Diff-Count "$wd\Testing\mx3-14-idle.png" "$wd\Testing\mx3-30-jp-hold.png"))
Log ("F: jplay stdout: " + (Get-Content "$wd\Testing\jplay-out.txt" -ErrorAction SilentlyContinue -Raw))
Start-Sleep -Seconds 2
try { Stop-Process -Id $llProc.Id -Force -ErrorAction SilentlyContinue } catch { }

# ---- G: cleanup ----
Log ("G: final mods=" + [Mx3]::ModifierStates() + " fg='" + [Mx3]::FgTitle() + "'")
[void][Mx3]::Tap(0x1B)
Log "G: run3 done"
