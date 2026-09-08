$ErrorActionPreference = 'Continue'
$wd = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp6 {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder s, int n);
    public static string ForegroundTitle() {
        var sb = new System.Text.StringBuilder(256);
        GetWindowText(GetForegroundWindow(), sb, 256);
        return sb.ToString();
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

function Click-WindowCenter([string]$procName) {
    $p = Get-Process $procName -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $p) { Write-Host "no window: $procName"; return }
    $r = New-Object Exp6+RECT
    [Exp6]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
    $cx = [int](($r.L + $r.R) / 2); $cy = [int](($r.T + $r.B) / 2)
    [Exp6]::SetCursorPos($cx, $cy) | Out-Null
    Start-Sleep -Milliseconds 150
    [Exp6]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [Exp6]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 700
    Write-Host ("focused " + $procName + " -> fg='" + [Exp6]::ForegroundTitle() + "'")
}

Write-Host "=== TEST A: InputInjector RightAlt hold (notepad focused) ==="
try {
    [void][Windows.UI.Input.Preview.Injection.InputInjector, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    [void][Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions, Windows.UI.Input.Preview.Injection, ContentType=WindowsRuntime]
    $injector = [Windows.UI.Input.Preview.Injection.InputInjector]::TryCreate()
    if ($injector) {
        Click-WindowCenter "notepad"
        $down = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
        $down.VirtualKey = [uint16]165
        $injector.InjectKeyboardInput(@($down))
        Start-Sleep -Milliseconds 800
        $held = (([Exp6]::GetAsyncKeyState(0xA5)) -band 0x8000) -ne 0
        Write-Host "inputinjector rmenu down observed: $held"
        Save-Shot "$wd\Testing\ii-during-hold.png"
        $up = [Windows.UI.Input.Preview.Injection.InjectedInputKeyboardInfo]::new()
        $up.VirtualKey = [uint16]165
        $up.KeyOptions = [Windows.UI.Input.Preview.Injection.InjectedInputKeyOptions]::KeyUp
        $injector.InjectKeyboardInput(@($up))
        Start-Sleep -Milliseconds 500
        Write-Host "inputinjector hold released"
    } else {
        Write-Host "inputinjector TryCreate returned null"
    }
} catch {
    Write-Host ("inputinjector failed: " + $_.Exception.Message)
}

Write-Host "=== TEST B: UIA scan of Doubao processes ==="
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
foreach ($procName in @('DoubaoImeSettings','ImeService','ImeWatchdog')) {
    $procs = Get-Process $procName -ErrorAction SilentlyContinue
    foreach ($p in $procs) {
        $wnds = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($w in $wnds) {
            if ($w.Current.ProcessId -eq $p.Id) {
                Write-Host ("doubao window: pid=" + $p.Id + " class=" + $w.Current.ClassName + " name='" + $w.Current.Name + "'")
            }
        }
    }
}

Write-Host "=== TEST C: Win+H with real focus ==="
Click-WindowCenter "notepad"
Save-Shot "$wd\Testing\winh2-before.png"
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WinH {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint INPUT_KEYBOARD = 1;
    static INPUT Vk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static void WinH() {
        INPUT[] list = new INPUT[] { Vk(0x5B, 0), Vk(0x48, 0), Vk(0x48, KEYEVENTF_KEYUP), Vk(0x5B, KEYEVENTF_KEYUP) };
        uint sent = SendInput((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT)));
        System.Console.WriteLine("winh sent=" + sent);
    }
}
'@
[WinH]::WinH()
Start-Sleep -Milliseconds 2500
Save-Shot "$wd\Testing\winh2-after.png"
Write-Host "winh2 saved"
