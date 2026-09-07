# Step 4e: WM_QUIT via Toolhelp32 thread snapshot. ASCII only.
$ErrorActionPreference = 'SilentlyContinue'

$proc = Get-Process -Name 'sayall-windows-app'
if (-not $proc) { Write-Output 'NOT_RUNNING'; exit 0 }
Write-Output "PID=$($proc.Id)"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class ThreadQuit {
    [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint tid, uint msg, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential)]
    public struct THREADENTRY32 { public uint dwSize, cntUsage, th32ThreadID, th32OwnerProcessID;
        public int tpBasePri, tpDeltaPri, dwFlags; }
    [DllImport("kernel32.dll")] public static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid2);
    [DllImport("kernel32.dll")] public static extern bool Thread32First(IntPtr h, ref THREADENTRY32 e);
    [DllImport("kernel32.dll")] public static extern bool Thread32Next(IntPtr h, ref THREADENTRY32 e);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);

    public static uint[] ThreadsOf(uint pid2) {
        IntPtr h = CreateToolhelp32Snapshot(0x00000004, pid2);
        if (h == (IntPtr)(-1)) return new uint[0];
        var list = new System.Collections.Generic.List<uint>();
        THREADENTRY32 e = new THREADENTRY32();
        e.dwSize = (uint)Marshal.SizeOf(typeof(THREADENTRY32));
        if (Thread32First(h, ref e)) {
            do { if (e.th32OwnerProcessID == pid2) list.Add(e.th32ThreadID); }
            while (Thread32Next(h, ref e));
        }
        CloseHandle(h);
        return list.ToArray();
    }
}
"@

$tids = [ThreadQuit]::ThreadsOf([uint32]$proc.Id)
Write-Output "THREADS=$($tids.Count) ids=$($tids -join ',')"
if ($tids.Count -eq 0) { Write-Output 'NO_THREADS'; exit 2 }

# try every thread with WM_QUIT (only threads with a message loop will act on it)
foreach ($tid in $tids) {
    $ok = [ThreadQuit]::PostThreadMessage($tid, 0x0012, [IntPtr]::Zero, [IntPtr]::Zero)
    Write-Output "WM_QUIT tid=$tid result=$ok"
}
Start-Sleep -Seconds 5
if (Get-Process -Name 'sayall-windows-app') { Write-Output 'STILL_RUNNING'; exit 3 }
Write-Output 'APP_EXITED'
