$ErrorActionPreference = 'Continue'
$wd = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ts = Get-Date -Format 'HHmmss'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Mx5 {
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hWnd, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder s, int n);
    public static string FgTitle() {
        var sb = new StringBuilder(256); GetWindowText(GetForegroundWindow(), sb, 256); return sb.ToString();
    }
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
    public static bool RAltDown { get { return (GetAsyncKeyState(0xA5) & 0x8000) != 0; } }
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

Log ("jp3 start ts=$ts fg='" + [Mx5]::FgTitle() + "' doubao=" + [Mx5]::DoubaoWinStates())
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object StartTime -Descending | Select-Object -First 1
if ($np) { [Mx5]::SetForegroundWindow($np.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 700 }
Log ("fg='" + [Mx5]::FgTitle() + "' doubao=" + [Mx5]::DoubaoWinStates())

$llmonLog = "$wd\Testing\llmon-jp3-$ts.log"
$llProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',$llmonLog,'-Seconds','60' -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 2
Log ("pre-play doubao=" + [Mx5]::DoubaoWinStates())
Save-Shot "$wd\Testing\mx4-10-pre.png"

Log "launching jp.exe 400 1400 (native journal playback RAlt)"
$jpProc = Start-Process "$wd\Testing\jp\jp.exe" -ArgumentList '400','1400' -WindowStyle Hidden -PassThru -RedirectStandardOutput "$wd\Testing\jp3-out.txt" -RedirectStandardError "$wd\Testing\jp3-err.txt"
Start-Sleep -Milliseconds 1250
Log ("mid-hold raltAsync=" + [Mx5]::RAltDown + " doubao=" + [Mx5]::DoubaoWinStates())
Save-Shot "$wd\Testing\mx4-11-jp-hold.png"
$exited = $jpProc.WaitForExit(6000)
Log ("jp exited=$exited raltAfter=" + [Mx5]::RAltDown + " doubao=" + [Mx5]::DoubaoWinStates())
Start-Sleep -Milliseconds 1500
Log ("post doubao=" + [Mx5]::DoubaoWinStates())
Save-Shot "$wd\Testing\mx4-12-jp-after.png"
Log ("jp3 stdout: " + (Get-Content "$wd\Testing\jp3-out.txt" -Raw -ErrorAction SilentlyContinue))
Start-Sleep -Seconds 2
try { Stop-Process -Id $llProc.Id -Force -ErrorAction SilentlyContinue } catch { }
Log '--- llmon-jp3 log ---'
Get-Content $llmonLog -ErrorAction SilentlyContinue
