$ErrorActionPreference = 'Continue'
$wd = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp7 {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder s, int n);
    public static string ForegroundTitle() {
        var sb = new System.Text.StringBuilder(256);
        GetWindowText(GetForegroundWindow(), sb, 256);
        return sb.ToString();
    }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint INPUT_KEYBOARD = 1;
    static INPUT Vk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static uint PressWinH() {
        INPUT[] list = new INPUT[] { Vk(0x5B, 0), Vk(0x48, 0), Vk(0x48, KEYEVENTF_KEYUP), Vk(0x5B, KEYEVENTF_KEYUP) };
        return SendInput((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@

function Save-Shot([string]$path) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

function Focus-Notepad() {
    $p = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $p) { Write-Host "notepad missing"; return $false }
    [Exp7]::SetWindowPos($p.MainWindowHandle, [IntPtr]::Zero, 60, 60, 900, 600, 0x0040) | Out-Null
    Start-Sleep -Milliseconds 400
    [Exp7]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 400
    $r = New-Object Exp7+RECT
    [Exp7]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
    $cx = [int](($r.L + $r.R) / 2); $cy = [int](($r.T + $r.B) / 2)
    [Exp7]::SetCursorPos($cx, $cy) | Out-Null
    Start-Sleep -Milliseconds 150
    [Exp7]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [Exp7]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 700
    $fg = [Exp7]::ForegroundTitle()
    Write-Host ("fg='" + $fg + "'")
    return $fg -match '记事本|Notepad'
}

Write-Host "=== TEST A: InputInjector RightAlt hold ==="
try {
    [void][Windows.UI.Input.Preview.Injection.InputInjector, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    $injector = [Windows.UI.Input.Preview.Injection.InputInjector]::TryCreate()
    if ($injector) {
        $focused = Focus-Notepad
        $info = New-Object 'Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo[]' 1
        $down = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
        $down.VirtualKey = [uint16]165
        $down.ScanCode = [uint16]56
        $info[0] = $down
        $injector.InjectKeyboardInput($info)
        Start-Sleep -Milliseconds 800
        $held = (([Exp7]::GetAsyncKeyState(0xA5)) -band 0x8000) -ne 0
        Write-Host ("inputinjector held=" + $held + " focused=" + $focused)
        Save-Shot "$wd\Testing\ii2-during-hold.png"
        $up = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
        $up.VirtualKey = [uint16]165
        $up.ScanCode = [uint16]56
        $up.KeyOptions = [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::KeyUp -bor [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::ExtendedKey
        $info[0] = $up
        $injector.InjectKeyboardInput($info)
        Write-Host "inputinjector released"
    } else {
        Write-Host "inputinjector null"
    }
} catch {
    Write-Host ("inputinjector error: " + $_.Exception.Message)
}

Write-Host "=== TEST C: Win+H with positioned notepad ==="
$focused = Focus-Notepad
Save-Shot "$wd\Testing\winh3-before.png"
$sent = [Exp7]::PressWinH()
Start-Sleep -Milliseconds 2500
Save-Shot "$wd\Testing\winh3-after.png"
Write-Host ("winh sent=" + $sent + " focused=" + $focused)

Write-Host "=== TEST D: UIA tree of notepad after focus (looking for IME elements) ==="
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$np = Get-Process notepad | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$npElem = [System.Windows.Automation.AutomationElement]::FromHandle($np.MainWindowHandle)
$all = $npElem.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
$count = 0
foreach ($e in $all) {
    if ($e.Current.ProcessId -ne $np.Id) {
        Write-Host ("foreign elem: pid=" + $e.Current.ProcessId + " class=" + $e.Current.ClassName + " name='" + $e.Current.Name + "'")
        $count++
    }
}
Write-Host ("foreign elements in notepad tree: " + $count)
