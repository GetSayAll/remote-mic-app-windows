$ErrorActionPreference = 'Continue'
$wd = "C:\Users\Administrator\Documents\Codex\remote-mic-app-windows"
$ts = Get-Date -Format 'HHmmss'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Mx {
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
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("advapi32.dll")] public static extern bool OpenProcessToken(IntPtr h, uint access, out IntPtr tok);
    [DllImport("advapi32.dll")] public static extern bool GetTokenInformation(IntPtr tok, int cls, IntPtr info, int len, out int retLen);
    [DllImport("advapi32.dll")] public static extern uint GetSidSubAuthority(IntPtr sid, int idx);
    [DllImport("advapi32.dll")] public static extern IntPtr GetSidSubAuthorityAuthority(IntPtr sid); // unused
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
    const uint KEYUP = 0x2;
    const uint EXT = 0x1;
    const uint SCAN = 0x8;
    const uint UNICODE = 0x4;
    static INPUT Mk(ushort vk, ushort scan, uint flags, ulong extra) {
        INPUT i = new INPUT(); i.type = 1;
        i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags;
        i.u.ki.dwExtraInfo = (IntPtr)extra;
        return i;
    }
    public static uint Tap(ushort vk, uint flags, ulong extra) {
        INPUT[] a = new INPUT[] { Mk(vk, 0, flags, extra), Mk(vk, 0, flags | KEYUP, extra) };
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Down(ushort vk, ushort scan, uint flags, ulong extra) {
        return SendInput(1, new INPUT[] { Mk(vk, scan, flags, extra) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Up(ushort vk, ushort scan, uint flags, ulong extra) {
        return SendInput(1, new INPUT[] { Mk(vk, scan, flags | KEYUP, extra) }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint Batch(ushort[][] spec) {
        INPUT[] a = new INPUT[spec.Length];
        for (int i = 0; i < spec.Length; i++) a[i] = Mk(spec[i][0], spec[i][1], (uint)spec[i][2], 0);
        return SendInput((uint)a.Length, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint UniTap(string text) {
        INPUT[] a = new INPUT[text.Length * 2];
        int k = 0;
        foreach (char c in text) {
            a[k++] = Mk(0, (ushort)c, UNICODE, 0);
            a[k++] = Mk(0, (ushort)c, UNICODE | KEYUP, 0);
        }
        return SendInput((uint)a.Length, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static string FgTitle() {
        var sb = new System.Text.StringBuilder(256);
        GetWindowText(GetForegroundWindow(), sb, 256);
        return sb.ToString();
    }
    public static string FgInfo() {
        IntPtr fg = GetForegroundWindow();
        uint pid; GetWindowThreadProcessId(fg, out pid);
        string il = "?";
        try {
            IntPtr hProc = OpenProcess(0x1000, false, pid);
            IntPtr tok;
            if (hProc != IntPtr.Zero && OpenProcessToken(hProc, 0x8, out tok)) {
                int len;
                GetTokenInformation(tok, 25, IntPtr.Zero, 0, out len);
                IntPtr buf = Marshal.AllocHGlobal(len);
                if (GetTokenInformation(tok, 25, buf, len, out len)) {
                    IntPtr sid = Marshal.ReadIntPtr(buf);
                    uint rid = GetSidSubAuthority(sid, 0);
                    if (rid < 0x2000) il = "Low"; else if (rid < 0x4000) il = "Medium"; else if (rid < 0x5000) il = "High"; else il = "System";
                }
                Marshal.FreeHGlobal(buf);
            }
        } catch { }
        return "pid=" + pid + " il=" + il + " fg='" + FgTitle() + "'";
    }
    public static bool RAltDown { get { return (GetAsyncKeyState(0xA5) & 0x8000) != 0; } }
    public static bool LWinDown { get { return (GetAsyncKeyState(0x5B) & 0x8000) != 0; } }
    public static string ModifierStates() {
        int[] vks = new int[] { 0x5B, 0x5C, 0x10, 0xA0, 0xA1, 0x11, 0xA2, 0xA3, 0x12, 0xA4, 0xA5, 0x20 };
        string[] names = new string[] { "LWin", "RWin", "Shift", "LShift", "RShift", "Ctrl", "LCtrl", "RCtrl", "Alt", "LAlt", "RAlt", "Space" };
        var sb = new System.Text.StringBuilder();
        for (int i = 0; i < vks.Length; i++) {
            bool d = (GetAsyncKeyState(vks[i]) & 0x8000) != 0;
            if (d) { sb.Append(names[i]); sb.Append("=DOWN "); }
        }
        return sb.Length == 0 ? "(none)" : sb.ToString();
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
function Marker([int]$n) { # F13..F20 marker taps
    $vk = [ushort](0x7B + $n)
    $r = [Mx]::Tap($vk, 0, 0)
    Log ("marker F" + ($n + 12) + " vk=0x" + $vk.ToString('X') + " sent=" + $r)
}

Log ("run start ts=" + $ts + " fginfo=" + [Mx]::FgInfo())

# ---- Phase 0: start LL/raw monitor in background ----
$llmonProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',"$wd\Testing\llmon-matrix-$ts.log",'-Seconds','150' -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 2

# ---- Phase 1: modifier residue check + cleanup ----
Log ("PHASE1 residue check: " + [Mx]::ModifierStates())
foreach ($k in @(@(0x5B, 0), @(0x5C, 0), @(0x10, 0), @(0xA0, 0), @(0xA1, 0), @(0x11, 0), @(0xA2, 0), @(0xA3, 0), @(0x12, 0), @(0xA4, 0), @(0xA5, 0))) {
    $vk = $k[0]
    if ((([Mx]::GetAsyncKeyState($vk)) -band 0x8000) -ne 0) {
        $ext = 0; if ($vk -eq 0xA5 -or $vk -eq 0xA3 -or $vk -eq 0xA1 -or $vk -eq 0x5C) { $ext = 1 }
        $r = [Mx]::Up([ushort]$vk, 0, [uint32](2 -bor $ext), 0)
        Log ("cleaned residue vk=0x" + $vk.ToString('X') + " sent=" + $r)
    }
}
Log ("PHASE1 after cleanup: " + [Mx]::ModifierStates())

# ---- Phase 2: IME default override -> Doubao, fresh notepad ----
$prevOverride = Get-WinDefaultInputMethodOverride
Log ("PHASE2 prev override: '" + $prevOverride + "'")
Set-WinDefaultInputMethodOverride -InputTip "0804:{9D2B2E2B-3C93-4D2F-9D35-6EEB85F0D2B0}{2B4D4B3A-4D4F-4C0A-8E66-7F771A2B9C10}"
Log ("PHASE2 override now: '" + (Get-WinDefaultInputMethodOverride) + "'")
$npProc = Start-Process notepad -PassThru
$deadline = (Get-Date).AddSeconds(5)
while (-not $npProc.MainWindowHandle -or $npProc.MainWindowHandle -eq 0) {
    Start-Sleep -Milliseconds 200
    $npProc.Refresh()
    if ((Get-Date) -gt $deadline) { break }
}
$npHwnd = $npProc.MainWindowHandle
Log ("PHASE2 notepad pid=" + $npProc.Id + " hwnd=0x" + $npHwnd.ToString('X'))
[Mx]::SetWindowPos($npHwnd, [IntPtr]::Zero, 60, 60, 900, 600, 0x0040) | Out-Null
Start-Sleep -Milliseconds 500
[Mx]::SetForegroundWindow($npHwnd) | Out-Null
Start-Sleep -Milliseconds 500
$r = New-Object Mx+RECT
[Mx]::GetWindowRect($npHwnd, [ref]$r) | Out-Null
$cx = [int](($r.L + $r.R) / 2); $cy = [int](($r.T + $r.B) / 2)
[Mx]::SetCursorPos($cx, $cy) | Out-Null
Start-Sleep -Milliseconds 150
[Mx]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
[Mx]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 900
Log ("PHASE2 focus: " + [Mx]::FgInfo())
Save-Shot "$wd\Testing\mx-00-notepad-baseline.png"
$editHwnd = [Mx]::FindWindowEx($npHwnd, [IntPtr]::Zero, "Edit", $null)
Log ("PHASE2 notepad edit hwnd=0x" + $editHwnd.ToString('X'))

# ---- Phase 3: MECH-A SendInput wVk-only RightAlt hold ----
Marker 1
Log "PHASE3 MECH-A wVk-only RAlt hold start"
$a1 = [Mx]::Down(0xA5, 0, 0, 0)
Start-Sleep -Milliseconds 400
$heldA1 = [Mx]::RAltDown
Start-Sleep -Milliseconds 500
Save-Shot "$wd\Testing\mx-01-wvk-hold.png"
Start-Sleep -Milliseconds 500
$heldA2 = [Mx]::RAltDown
$a2 = [Mx]::Up(0xA5, 0, 2, 0)
Log ("PHASE3 MECH-A sentDown=$a1 held@400=$heldA1 held@1400=$heldA2 sentUp=$a2")
Start-Sleep -Milliseconds 1200
Save-Shot "$wd\Testing\mx-01-wvk-after.png"
Log ("PHASE3 diff baseline-vs-hold: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-01-wvk-hold.png"))

# ---- Phase 4: MECH-B SendInput scan+ext RightAlt hold ----
Marker 2
Log "PHASE4 MECH-B scan+ext RAlt hold start"
$b1 = [Mx]::Down(0, 0x38, 0x9, 0)
Start-Sleep -Milliseconds 400
$heldB1 = [Mx]::RAltDown
Start-Sleep -Milliseconds 500
Save-Shot "$wd\Testing\mx-02-scan-hold.png"
Start-Sleep -Milliseconds 500
$heldB2 = [Mx]::RAltDown
$b2 = [Mx]::Up(0, 0x38, 0xB, 0)
Log ("PHASE4 MECH-B sentDown=$b1 held@400=$heldB1 held@1400=$heldB2 sentUp=$b2")
Start-Sleep -Milliseconds 1200
Save-Shot "$wd\Testing\mx-02-scan-after.png"
Log ("PHASE4 diff baseline-vs-hold: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-02-scan-hold.png"))

# ---- Phase 5: MECH-C InputInjector RightAlt hold ----
Marker 3
Log "PHASE5 MECH-C InputInjector RAlt hold start"
try {
    [void][Windows.UI.Input.Preview.Injection.InputInjector, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    $injector = [Windows.UI.Input.Preview.Injection.InputInjector]::TryCreate()
    if ($injector) {
        $info = New-Object 'Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo[]' 1
        $down = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
        $down.VirtualKey = [uint16]165
        $down.ScanCode = [uint16]56
        $info[0] = $down
        $injector.InjectKeyboardInput($info)
        Start-Sleep -Milliseconds 400
        $heldC1 = [Mx]::RAltDown
        Start-Sleep -Milliseconds 500
        Save-Shot "$wd\Testing\mx-03-ii-hold.png"
        Start-Sleep -Milliseconds 500
        $heldC2 = [Mx]::RAltDown
        $up = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
        $up.VirtualKey = [uint16]165
        $up.ScanCode = [uint16]56
        $up.KeyOptions = [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::KeyUp -bor [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::ExtendedKey
        $info[0] = $up
        $injector.InjectKeyboardInput($info)
        Log ("PHASE5 MECH-C held@400=$heldC1 held@1400=$heldC2 (released)")
    } else { Log "PHASE5 InputInjector TryCreate null" }
} catch { Log ("PHASE5 InputInjector error: " + $_.Exception.Message) }
Start-Sleep -Milliseconds 1200
Save-Shot "$wd\Testing\mx-03-ii-after.png"
Log ("PHASE5 diff baseline-vs-hold: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-03-ii-hold.png"))

# ---- Phase 6: MECH-D PostMessage WM_SYSKEYDOWN to notepad edit ----
Marker 4
Log "PHASE6 MECH-D PostMessage RAlt to notepad edit"
if ($editHwnd -ne [IntPtr]::Zero) {
    [Mx]::PostMessage($editHwnd, 0x0104, [IntPtr]0xA5, [IntPtr]0x21380001) | Out-Null
    Log ("PHASE6 posted WM_SYSKEYDOWN, fg=" + [Mx]::FgTitle())
    Start-Sleep -Milliseconds 900
    Save-Shot "$wd\Testing\mx-04-post-hold.png"
    [Mx]::PostMessage($editHwnd, 0x0105, [IntPtr]0xA5, [IntPtr]0xE1380001) | Out-Null
    Log ("PHASE6 posted WM_SYSKEYUP, asyncRAlt=" + [Mx]::RAltDown)
} else { Log "PHASE6 no edit hwnd" }
Start-Sleep -Milliseconds 1000
Save-Shot "$wd\Testing\mx-04-post-after.png"
Log ("PHASE6 diff baseline-vs-hold: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-04-post-hold.png"))

# ---- Phase 7: injected 'a' -> Doubao composition? ----
Marker 5
Log "PHASE7 inject 'A' tap to see IME composition"
$pa = [Mx]::Tap(0x41, 0, 0)
Start-Sleep -Milliseconds 700
Save-Shot "$wd\Testing\mx-05-composition.png"
Log ("PHASE7 A-tap sent=$pa fg=" + [Mx]::FgTitle())
[void][Mx]::Tap(0x1B, 0, 0)
Start-Sleep -Milliseconds 300
[void][Mx]::Tap(0x1B, 0, 0)
Log ("PHASE7 diff baseline-vs-composition: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-05-composition.png"))

# ---- Phase 8: Win+H per-event ----
Marker 6
Log ("PHASE8 Win+H per-event, pre: " + [Mx]::FgInfo() + " mods=" + [Mx]::ModifierStates())
Save-Shot "$wd\Testing\mx-06-winh-before.png"
$w1 = [Mx]::Down(0x5B, 0, 0, 0); $g1 = [Mx]::GetLastError()
Start-Sleep -Milliseconds 80
$w2 = [Mx]::Down(0x48, 0, 0, 0); $g2 = [Mx]::GetLastError()
Start-Sleep -Milliseconds 60
$w3 = [Mx]::Up(0x48, 0, 2, 0); $g3 = [Mx]::GetLastError()
Start-Sleep -Milliseconds 60
$w4 = [Mx]::Up(0x5B, 0, 2, 0); $g4 = [Mx]::GetLastError()
Log ("PHASE8 per-event ret: LWinD=$w1(gle=$g1) HD=$w2(gle=$g2) HU=$w3(gle=$g3) LWinU=$w4(gle=$g4)")
Start-Sleep -Milliseconds 2500
Save-Shot "$wd\Testing\mx-06-winh-per.png"
Log ("PHASE8 diff: " + (Diff-Count "$wd\Testing\mx-06-winh-before.png" "$wd\Testing\mx-06-winh-per.png"))
[void][Mx]::Tap(0x1B, 0, 0)
Start-Sleep -Milliseconds 700

# ---- Phase 9: Win+H batch-4 ----
Marker 7
Log ("PHASE9 Win+H batch4, pre: " + [Mx]::FgInfo() + " mods=" + [Mx]::ModifierStates())
$spec = @(
    ,@([ushort]0x5B, [ushort]0, [uint32]0)
    ,@([ushort]0x48, [ushort]0, [uint32]0)
    ,@([ushort]0x48, [ushort]0, [uint32]2)
    ,@([ushort]0x5B, [ushort]0, [uint32]2)
)
$b4ret = [Mx]::Batch($spec)
$g5 = [Mx]::GetLastError()
Log ("PHASE9 batch4 sent=$b4ret gle=$g5")
Start-Sleep -Milliseconds 2500
Save-Shot "$wd\Testing\mx-07-winh-batch.png"
Log ("PHASE9 diff: " + (Diff-Count "$wd\Testing\mx-06-winh-before.png" "$wd\Testing\mx-07-winh-batch.png"))
[void][Mx]::Tap(0x1B, 0, 0)
Start-Sleep -Milliseconds 700

# ---- Phase 10: Alt+Space system menu ----
Marker 8
Log "PHASE10 Alt+Space system menu"
$altD = [Mx]::Down(0x12, 0, 0, 0)
Start-Sleep -Milliseconds 150
$spT = [Mx]::Tap(0x20, 0, 0)
Start-Sleep -Milliseconds 600
Save-Shot "$wd\Testing\mx-08-altspace.png"
$altU = [Mx]::Up(0x12, 0, 2, 0)
[void][Mx]::Tap(0x1B, 0, 0)
Log ("PHASE10 altD=$altD spaceTap=$spT altU=$altU")
Log ("PHASE10 diff: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-08-altspace.png"))
Start-Sleep -Milliseconds 600

# ---- Phase 11: KEYEVENTF_UNICODE text injection ----
Marker 9
Log "PHASE11 KEYEVENTF_UNICODE 'nihao' into notepad"
$uniRet = [Mx]::UniTap("nihao")
Start-Sleep -Milliseconds 900
Save-Shot "$wd\Testing\mx-09-unicode.png"
Log ("PHASE11 unicode sent=$uniRet fg=" + [Mx]::FgTitle())
Log ("PHASE11 diff baseline: " + (Diff-Count "$wd\Testing\mx-00-notepad-baseline.png" "$wd\Testing\mx-09-unicode.png"))

# ---- Cleanup ----
Log ("cleanup: mods=" + [Mx]::ModifierStates())
try {
    if ([string]::IsNullOrEmpty($prevOverride)) { Set-WinDefaultInputMethodOverride } else { Set-WinDefaultInputMethodOverride -InputTip $prevOverride }
    Log ("override restored to: '" + (Get-WinDefaultInputMethodOverride) + "'")
} catch { Log ("override restore error: " + $_.Exception.Message) }
Start-Sleep -Seconds 3
try { Stop-Process -Id $llmonProc.Id -Force -ErrorAction SilentlyContinue } catch { }
Log ("run done. llmon log: llmon-matrix-$ts.log")
