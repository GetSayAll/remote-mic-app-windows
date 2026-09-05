# 启动生产版 exe，把它的窗口移到 (0,0) 并置顶，输出 PID。
param(
    [Parameter(Mandatory = $true)][string]$ExePath
)

Add-Type @'
using System;
using System.Runtime.InteropServices;
public class WMove2 {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr l);
  public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
'@

$p = Start-Process -FilePath $ExePath -PassThru
Start-Sleep -Seconds 6
$targetPid = $p.Id
$main = [IntPtr]::Zero
$found = $false
$callback = [WMove2+EnumWindowsProc]{
    param($h, $l)
    $tp = [uint32]0
    [WMove2]::GetWindowThreadProcessId($h, [ref]$tp) | Out-Null
    if ([int]$tp -eq $targetPid -and [WMove2]::IsWindowVisible($h)) {
        $script:main = $h
        return $false
    }
    return $true
}
[WMove2]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
if ($main -eq [IntPtr]::Zero) { throw "未找到 PID=$targetPid 的可见窗口" }
$topmost = New-Object System.IntPtr(-1)
[WMove2]::SetWindowPos($main, $topmost, 0, 0, 0, 0, 0x0003) | Out-Null
Write-Host "app pid: $targetPid"
$targetPid | Out-File -Encoding ascii "$env:TEMP\sayall-myapp-pid.txt"
