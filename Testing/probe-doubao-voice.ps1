$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint KEYEVENTF_EXTENDEDKEY = 0x1;
    const uint KEYEVENTF_SCANCODE = 0x8;
    const uint INPUT_KEYBOARD = 1;
    public static INPUT Vk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static INPUT Scan(ushort scan, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; return i; }
    public static uint Batch(INPUT[] list) { return SendInput((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT))); }
    public static void RightAltDown() { Batch(new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY) }); }
    public static void RightAltUp() { Batch(new INPUT[] { Scan(0x38, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP) }); }
    public static void KeyTap(ushort vk, bool extended) {
        uint flags = extended ? KEYEVENTF_EXTENDEDKEY : 0;
        Batch(new INPUT[] { Vk(vk, flags), Vk(vk, flags | KEYEVENTF_KEYUP) });
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

function Diff-Count([string]$a, [string]$b) {
    $imgA = [System.Drawing.Bitmap]::FromFile($a)
    $imgB = [System.Drawing.Bitmap]::FromFile($b)
    $w = [Math]::Min($imgA.Width, $imgB.Width); $h = [Math]::Min($imgA.Height, $imgB.Height)
    $diff = 0
    for ($y = 0; $y -lt $h; $y += 3) {
        for ($x = 0; $x -lt $w; $x += 3) {
            $pa = $imgA.GetPixel($x, $y); $pb = $imgB.GetPixel($x, $y)
            if ([Math]::Abs($pa.R - $pb.R) + [Math]::Abs($pa.G - $pb.G) + [Math]::Abs($pa.B - $pb.B) -gt 30) { $diff++ }
        }
    }
    $imgA.Dispose(); $imgB.Dispose()
    return $diff
}

# 1. Focus Notepad
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $np) { Start-Process notepad; Start-Sleep -Seconds 2; $np = Get-Process notepad | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1 }
[Exp]::SetForegroundWindow($np.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 900
Save-Shot "Testing\exp-0-baseline.png"
Write-Host "focused notepad"

# 2. Cycle input method with Win+Space
[Exp]::Batch(@([Exp]::Vk(0x5B, 0))) | Out-Null
[Exp]::KeyTap(0x20, $false)
[Exp]::Batch(@([Exp]::Vk(0x5B, 2))) | Out-Null
Start-Sleep -Milliseconds 1200
Save-Shot "Testing\exp-1-after-winspace.png"

# 3. Hold Right Alt for 1.5s, screenshot during hold
[Exp]::RightAltDown()
Start-Sleep -Milliseconds 1500
Save-Shot "Testing\exp-2-during-hold.png"
[Exp]::RightAltUp()
Start-Sleep -Milliseconds 1200
Save-Shot "Testing\exp-3-after-release.png"

# 4. Try Right Alt + Space tap variant
[Exp]::RightAltDown()
Start-Sleep -Milliseconds 120
[Exp]::KeyTap(0x20, $false)
Start-Sleep -Milliseconds 120
[Exp]::RightAltUp()
Start-Sleep -Milliseconds 1500
Save-Shot "Testing\exp-4-alt-space.png"

Write-Host ("diff baseline-vs-winspace: " + (Diff-Count "Testing\exp-0-baseline.png" "Testing\exp-1-after-winspace.png"))
Write-Host ("diff winspace-vs-during-hold: " + (Diff-Count "Testing\exp-1-after-winspace.png" "Testing\exp-2-during-hold.png"))
Write-Host ("diff during-hold-vs-after-release: " + (Diff-Count "Testing\exp-2-during-hold.png" "Testing\exp-3-after-release.png"))
Write-Host ("diff after-release-vs-alt-space: " + (Diff-Count "Testing\exp-3-after-release.png" "Testing\exp-4-alt-space.png"))
