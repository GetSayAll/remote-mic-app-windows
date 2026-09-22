# E1-a pre-probe: enumerate RC003 (VID 2717 / PID 32B8) raw input entries.
# Prints dwType / registered usage page+usage / device path.
$ErrorActionPreference = "Continue"
$outFile = "<PROBE-DIR>\rawdev-info.txt"

try {
  Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class RawDevInfo {
  [StructLayout(LayoutKind.Sequential)]
  public struct RAWINPUTDEVICELIST { public IntPtr hDevice; public uint dwType; }

  [DllImport("user32.dll")]
  public static extern uint GetRawInputDeviceList(IntPtr p, ref uint count, uint size);

  [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "GetRawInputDeviceInfoW")]
  public static extern uint GetRawInputDeviceInfo(IntPtr h, uint cmd, IntPtr data, ref uint len);

  public static string GetName(IntPtr h) {
    uint len = 0;
    GetRawInputDeviceInfo(h, 0x20000007, IntPtr.Zero, ref len);
    if (len == 0) return "";
    IntPtr buf = Marshal.AllocHGlobal((int)(len * 2));
    uint written = GetRawInputDeviceInfo(h, 0x20000007, buf, ref len);
    string name = (written == 0xFFFFFFFF) ? "" : Marshal.PtrToStringUni(buf);
    Marshal.FreeHGlobal(buf);
    return name;
  }

  public static string GetDevInfo(IntPtr h) {
    uint len = 0;
    GetRawInputDeviceInfo(h, 0x2000000C, IntPtr.Zero, ref len);
    if (len == 0) return "(no-deviceinfo)";
    IntPtr buf = Marshal.AllocHGlobal((int)len);
    uint written = GetRawInputDeviceInfo(h, 0x2000000C, buf, ref len);
    if (written == 0xFFFFFFFF) { Marshal.FreeHGlobal(buf); return "(query-failed)"; }
    uint cbSize = (uint)Marshal.ReadInt32(buf, 0);
    uint dwType = (uint)Marshal.ReadInt32(buf, 4);
    string s = "cbSize=" + cbSize + " type=" + dwType;
    if (dwType == 2) {
      uint vid = (uint)Marshal.ReadInt32(buf, 8);
      uint pid = (uint)Marshal.ReadInt32(buf, 12);
      uint ver = (uint)Marshal.ReadInt32(buf, 16);
      ushort up = (ushort)Marshal.ReadInt16(buf, 20);
      ushort uu = (ushort)Marshal.ReadInt16(buf, 22);
      s += String.Format(" HID vid=0x{0:X4} pid=0x{1:X4} ver={2} usagePage=0x{3:X2} usage=0x{4:X2}", vid, pid, ver, up, uu);
    }
    Marshal.FreeHGlobal(buf);
    return s;
  }

  public static string[] Dump() {
    uint size = (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICELIST));
    uint count = 0;
    GetRawInputDeviceList(IntPtr.Zero, ref count, size);
    if (count == 0) return new string[0];
    IntPtr ptr = Marshal.AllocHGlobal((int)(size * count));
    uint got = GetRawInputDeviceList(ptr, ref count, size);
    var res = new System.Collections.Generic.List<string>();
    for (uint i = 0; i < got; i++) {
      IntPtr ip = new IntPtr(ptr.ToInt64() + (size * i));
      RAWINPUTDEVICELIST it = (RAWINPUTDEVICELIST)Marshal.PtrToStructure(ip, typeof(RAWINPUTDEVICELIST));
      string n = GetName(it.hDevice);
      string lower = n.ToLowerInvariant();
      if (lower.IndexOf("2717") < 0 && lower.IndexOf("32b8") < 0) continue;
      res.Add("TYPE=" + it.dwType + " | " + GetDevInfo(it.hDevice) + " | path=" + n);
    }
    Marshal.FreeHGlobal(ptr);
    return res.ToArray();
  }
}
'@ -ErrorAction Stop

  $lines = [RawDevInfo]::Dump()
  $all = @("=== RC003 raw input entries: " + $lines.Count + " ===") + $lines
  $all | Out-File -FilePath $outFile -Encoding UTF8
  "OK written -> $outFile"
} catch {
  ("ERROR: " + $_.Exception.Message) | Out-File -FilePath $outFile -Encoding UTF8
  "FAILED: " + $_.Exception.Message
}
