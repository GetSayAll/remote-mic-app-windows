# 列出系统全部 Raw Input 设备（类型 + 设备路径）。
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class RawEnum3 {
  [StructLayout(LayoutKind.Sequential)]
  public struct RAWINPUTDEVICELIST { public IntPtr hDevice; public uint dwType; }
  [DllImport("user32.dll")]
  public static extern uint GetRawInputDeviceList(IntPtr p, ref uint count, uint size);
  [DllImport("user32.dll")]
  public static extern uint GetRawInputDeviceInfoW(IntPtr h, uint cmd, IntPtr data, ref uint len);
  public static string GetDevName(IntPtr h) {
    uint len = 0;
    GetRawInputDeviceInfoW(h, 0x20000007, IntPtr.Zero, ref len);
    if (len == 0) return "";
    IntPtr buf = Marshal.AllocHGlobal((int)(len * 2));
    uint written = GetRawInputDeviceInfoW(h, 0x20000007, buf, ref len);
    string name = written == 0xFFFFFFFF ? "" : Marshal.PtrToStringUni(buf);
    Marshal.FreeHGlobal(buf);
    return name;
  }
  public static string[] ListAll() {
    uint size = (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICELIST));
    uint count = 0;
    GetRawInputDeviceList(IntPtr.Zero, ref count, size);
    if (count == 0) return new string[0];
    IntPtr ptr = Marshal.AllocHGlobal((int)(size * count));
    GetRawInputDeviceList(ptr, ref count, size);
    var result = new System.Collections.Generic.List<string>();
    for (uint i = 0; i < count; i++) {
      IntPtr itemPtr = new IntPtr(ptr.ToInt64() + (size * i));
      RAWINPUTDEVICELIST item = (RAWINPUTDEVICELIST)Marshal.PtrToStructure(itemPtr, typeof(RAWINPUTDEVICELIST));
      string name = GetDevName(item.hDevice);
      if (name.Length == 0) { result.Add("TYPE=" + item.dwType + " <name-failed>"); continue; }
      result.Add("TYPE=" + item.dwType + " " + name);
    }
    Marshal.FreeHGlobal(ptr);
    return result.ToArray();
  }
}
'@
foreach ($d in [RawEnum3]::ListAll()) { Write-Output $d }
