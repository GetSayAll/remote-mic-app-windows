$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Exp5 {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint INPUT_KEYBOARD = 1;
    public static INPUT Vk(ushort vk, uint flags) { INPUT i = new INPUT(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = flags; return i; }
    public static uint Batch(INPUT[] list) { return SendInput((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT))); }
}
'@
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) { [Exp5]::SetForegroundWindow($np.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800 }
function Save-Shot([string]$path) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}
Save-Shot "Testing\winh-before.png"
# Win down, H tap, Win up
[Exp5]::Batch(@([Exp5]::Vk(0x5B, 0))) | Out-Null
Start-Sleep -Milliseconds 120
[Exp5]::Batch(@([Exp5]::Vk(0x48, 0), [Exp5]::Vk(0x48, 2))) | Out-Null
Start-Sleep -Milliseconds 120
[Exp5]::Batch(@([Exp5]::Vk(0x5B, 2))) | Out-Null
Start-Sleep -Milliseconds 2500
Save-Shot "Testing\winh-after.png"
Write-Host "winh test done"
