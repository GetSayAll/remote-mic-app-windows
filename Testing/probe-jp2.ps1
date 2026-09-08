$ErrorActionPreference = 'Continue'
$wd = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ts = Get-Date -Format 'HHmmss'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Mx4 {
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hWnd, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
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
            if (p == 9772) { // ImeService
                var cn = new StringBuilder(128); GetClassName(h, cn, 128);
                string c = cn.ToString();
                if (c.StartsWith("Oime") || c.StartsWith("Ome")) {
                    sb.Append(c + ":vis=" + IsWindowVisible(h) + " ");
                }
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

Log ("jp2 start ts=$ts fg='" + [Mx4]::FgTitle() + "' doubao=" + [Mx4]::DoubaoWinStates())
# ensure notepad foreground
$np = Get-Process notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object StartTime -Descending | Select-Object -First 1
if ($np) { [Mx4]::SetForegroundWindow($np.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 700 }
Log ("fg now='" + [Mx4]::FgTitle() + "' doubao=" + [Mx4]::DoubaoWinStates())

$llmonLog = "$wd\Testing\llmon-jp2-$ts.log"
$llProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\llmon.ps1",'-LogPath',$llmonLog,'-Seconds','60' -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 2
Log ("doubao pre-play: " + [Mx4]::DoubaoWinStates())
Save-Shot "$wd\Testing\mx4-00-pre.png"

Log "starting jplay (journal playback: RAlt down @400ms, hold 1400ms, up)"
$jpProc = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$wd\Testing\jplay.ps1",'-DownDelayMs','400','-HoldMs','1400' -WindowStyle Hidden -PassThru -RedirectStandardOutput "$wd\Testing\jplay2-out.txt" -RedirectStandardError "$wd\Testing\jplay2-err.txt"
Start-Sleep -Milliseconds 1200
Log ("mid-hold raltAsync=" + [Mx4]::RAltDown + " doubao=" + [Mx4]::DoubaoWinStates())
Save-Shot "$wd\Testing\mx4-01-jp-hold.png"
$exited = $jpProc.WaitForExit(6000)
Log ("jplay exited=$exited raltAfter=" + [Mx4]::RAltDown + " doubao=" + [Mx4]::DoubaoWinStates())
Start-Sleep -Milliseconds 1200
Log ("post doubao=" + [Mx4]::DoubaoWinStates())
Save-Shot "$wd\Testing\mx4-02-jp-after.png"
Log ("jplay2 stdout: " + (Get-Content "$wd\Testing\jplay2-out.txt" -Raw -ErrorAction SilentlyContinue))
Start-Sleep -Seconds 2
try { Stop-Process -Id $llProc.Id -Force -ErrorAction SilentlyContinue } catch { }
Log ("llmon log: " + $llmonLog)
Get-Content $llmonLog -ErrorAction SilentlyContinue
