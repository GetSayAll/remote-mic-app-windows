$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp2 {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint KEYEVENTF_EXTENDEDKEY = 0x1;
    const uint KEYEVENTF_SCANCODE = 0x8;
    const uint INPUT_KEYBOARD = 1;
    public static INPUT Vk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static INPUT Scan(ushort scan, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; return i; }
    public static uint Batch(INPUT[] list) { return SendInput((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT))); }
    public static void RightAltDown() { Batch(new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY) }); }
    public static void RightAltUp() { Batch(new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP) }); }
    public static bool RmenuDown { get { return (GetAsyncKeyState(0xA5) & 0x8000) != 0; } }
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
function Test-RightAltHold([string]$tag) {
    Save-Shot "Testing\r2-$tag-before.png"
    [Exp2]::RightAltDown()
    Start-Sleep -Milliseconds 1500
    $held = [Exp2]::RmenuDown
    Save-Shot "Testing\r2-$tag-hold.png"
    [Exp2]::RightAltUp()
    Start-Sleep -Milliseconds 1200
    Save-Shot "Testing\r2-$tag-after.png"
    $d = Diff-Count "Testing\r2-$tag-before.png" "Testing\r2-$tag-hold.png"
    Write-Host "$tag held=$held diff=$d"
}

# A. Doubao settings window focused (its own search box is a text field)
$ds = Get-Process DoubaoImeSettings -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($ds) { [Exp2]::SetForegroundWindow($ds.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 900; Test-RightAltHold "doubao-settings" }

# B. Notepad: type a letter first (engage IME), then Right Alt hold
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) {
    [Exp2]::SetForegroundWindow($np.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 900
    [Exp2]::Batch(@([Exp2]::Vk(0x4E, 0), [Exp2]::Vk(0x4E, 2))) | Out-Null
    Start-Sleep -Milliseconds 700
    Save-Shot "Testing\r2-notepad-typed.png"
    Test-RightAltHold "notepad-typed"
}
