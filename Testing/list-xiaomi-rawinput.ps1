# 枚举系统 Raw Input 设备，列出小米遥控器（VID 2717 / PID 32B8）相关路径。
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class RawEnum2 {
  [StructLayout(LayoutKind.Sequential)]
  public struct RAWINPUTDEVICELIST { public IntPtr hDevice; public uint dwType; }
  [DllImport("user32.dll")]
  public static extern uint GetRawInputDeviceList(IntPtr p, ref uint count, uint size);
  [DllImport("user32.dll")]
  public static extern uint GetRawInputDeviceInfoW(IntPtr h, uint cmd, StringBuilder name, ref uint len);
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
      uint len = 512; var sb = new StringBuilder(512);
      GetRawInputDeviceInfoW(item.hDevice, 0x20000007, sb, ref len);
      result.Add("TYPE=" + item.dwType + " " + sb.ToString());
    }
    Marshal.FreeHGlobal(ptr);
    return result.ToArray();
  }
}
'@
$all = [RawEnum2]::ListAll()
Write-Host "total raw devices: $($all.Count)"
foreach ($d in $all) { if ($d -match '2717|32b8') { Write-Host $d } }
