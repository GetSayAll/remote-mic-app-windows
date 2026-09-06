$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win32Probe {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint idThread);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder text, int count);
    public static string ActiveHklHex() {
        IntPtr hwnd = GetForegroundWindow();
        uint pid;
        uint tid = GetWindowThreadProcessId(hwnd, out pid);
        IntPtr hkl = GetKeyboardLayout(tid);
        return string.Format("hwnd=0x{0:X} pid={1} tid={2} hkl=0x{3:X}", hwnd.ToInt64(), pid, tid, hkl.ToInt64());
    }
}
'@
Write-Host ("FOREGROUND " + [Win32Probe]::ActiveHklHex())

$name = $args[0]
$proc = Get-Process -Name $name -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($proc) {
    [Win32Probe]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
    [Win32Probe]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 800
    $sb = New-Object System.Text.StringBuilder 256
    [Win32Probe]::GetWindowText($proc.MainWindowHandle, $sb, 256) | Out-Null
    Write-Host ("WINDOW '{0}' pid={1}" -f $sb.ToString(), $proc.Id)
}
Start-Sleep -Milliseconds 400
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$out = $args[1]
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Host "SAVED $out"
Write-Host ("AFTER " + [Win32Probe]::ActiveHklHex())
