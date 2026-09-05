# 捕获全系统键盘 Raw Input 事件（VK / 扫描码 / 设备路径），用于确定
# RC003 遥控器按键在 Windows 上的真实虚拟键与扫描码。
# 用法: capture-rawkeys.ps1 <输出文件> <运行秒数>
param(
    [Parameter(Mandatory = $true)][string]$OutFile,
    [Parameter(Mandatory = $true)][int]$Seconds
)

Add-Type @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public class RawKeyCap {
  [StructLayout(LayoutKind.Sequential)]
  public struct RAWINPUTDEVICE { public ushort usUsagePage; public ushort usUsage; public uint dwFlags; public IntPtr hwndTarget; }
  [StructLayout(LayoutKind.Sequential)]
  public struct RAWINPUTHEADER { public uint dwType; public uint dwSize; public IntPtr hDevice; public IntPtr wParam; }
  [StructLayout(LayoutKind.Sequential)]
  public struct RAWKEYBOARD { public ushort MakeCode; public ushort Flags; public ushort Reserved; public ushort VKey; public uint Message; public ulong ExtraInformation; }
  [StructLayout(LayoutKind.Explicit)]
  public struct RAWINPUT { [FieldOffset(0)] public RAWINPUTHEADER header; [FieldOffset(24)] public RAWKEYBOARD keyboard; }
  [StructLayout(LayoutKind.Sequential)]
  public struct WNDCLASSW { public uint style; public IntPtr lpfnWndProc; public int cbClsExtra; public int cbWndExtra; public IntPtr hInstance; public IntPtr hIcon; public IntPtr hCursor; public IntPtr hbrBackground; public IntPtr lpszMenuName; public IntPtr lpszClassName; }
  [StructLayout(LayoutKind.Sequential)]
  public struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public int ptX; public int ptY; }

  public delegate IntPtr WndProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
  public static WndProc ProcDelegate;

  [DllImport("user32.dll")] public static extern ushort RegisterClassW(ref WNDCLASSW wc);
  [DllImport("user32.dll")] public static extern IntPtr CreateWindowExW(uint exStyle, IntPtr className, IntPtr windowName, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
  [DllImport("user32.dll")] public static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] devices, uint count, uint size);
  [DllImport("user32.dll")] public static extern uint GetRawInputData(IntPtr hRawInput, uint uiCommand, IntPtr pData, ref uint pcbSize, uint cbSizeHeader);
  [DllImport("user32.dll")] public static extern uint GetRawInputDeviceInfoW(IntPtr h, uint cmd, IntPtr data, ref uint len);
  [DllImport("user32.dll")] public static extern bool GetMessageW(out MSG msg, IntPtr hwnd, uint min, uint max);
  [DllImport("user32.dll")] public static extern bool PeekMessageW(out MSG msg, IntPtr hwnd, uint min, uint max, uint remove);
  [DllImport("user32.dll")] public static extern IntPtr DispatchMessageW(ref MSG msg);
  [DllImport("user32.dll")] public static extern IntPtr DefWindowProcW(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
  [DllImport("kernel32.dll")] public static extern IntPtr GetModuleHandleW(IntPtr name);

  public const uint WM_INPUT = 0x00FF;
  public const uint WM_INPUT_DEVICE_CHANGE = 0x00FE;

  public static StreamWriter Writer;
  public static string LastDevice = "";

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

  public static IntPtr Proc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam) {
    if (msg == WM_INPUT) {
      uint size = 0;
      GetRawInputData(lParam, 0x10000003, IntPtr.Zero, ref size, (uint)Marshal.SizeOf(typeof(RAWINPUTHEADER)));
      if (size > 0 && size < 8192) {
        IntPtr buf = Marshal.AllocHGlobal((int)size);
        uint got = GetRawInputData(lParam, 0x10000003, buf, ref size, (uint)Marshal.SizeOf(typeof(RAWINPUTHEADER)));
        if (got > 0) {
          RAWINPUT raw = (RAWINPUT)Marshal.PtrToStructure(buf, typeof(RAWINPUT));
          string dev = GetDevName(raw.header.hDevice);
          bool xiaomi = dev.Contains("2717") || dev.Contains("32b8");
          string marker = xiaomi ? "XIAOMI" : "other";
          if (raw.header.dwType == 1) {
            string line = string.Format("{0} KEY vk=0x{1:X2} make=0x{2:X2} flags=0x{3:X2} msg=0x{4:X} dev={5}",
                marker, raw.keyboard.VKey, raw.keyboard.MakeCode, raw.keyboard.Flags, raw.keyboard.Message, dev);
            try { Writer.WriteLine(line); Writer.Flush(); } catch {}
          } else if (raw.header.dwType == 2) {
            // RAWHID：DWORD dwSizeHid, DWORD dwCount, BYTE bRawData[]
            uint sizeHid = (uint)Marshal.ReadInt32(buf, 24);
            uint count = (uint)Marshal.ReadInt32(buf, 28);
            byte[] data = new byte[(int)raw.header.dwSize - 32];
            Marshal.Copy(new IntPtr(buf.ToInt64() + 32), data, 0, data.Length);
            var hex = new StringBuilder();
            foreach (byte b in data) { if (hex.Length > 0) hex.Append(' '); hex.Append(b.ToString("X2")); }
            string line = string.Format("{0} HID sizeHid={1} count={2} data=[{3}] dev={4}",
                marker, sizeHid, count, hex.ToString(), dev);
            try { Writer.WriteLine(line); Writer.Flush(); } catch {}
          }
        }
        Marshal.FreeHGlobal(buf);
      }
      return IntPtr.Zero;
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
  }

  public static void Run(string path, int seconds) {
    Writer = new StreamWriter(path, true, Encoding.UTF8);
    Writer.WriteLine("--- capture start " + DateTime.Now.ToString("HH:mm:ss") + " ---");
    ProcDelegate = Proc; // 防止委托被 GC（原生函数指针指向的委托必须保持存活）
    IntPtr instance = GetModuleHandleW(IntPtr.Zero);
    WNDCLASSW wc = new WNDCLASSW();
    wc.lpfnWndProc = Marshal.GetFunctionPointerForDelegate(ProcDelegate);
    wc.hInstance = instance;
    string cls = "SayAllKeyCap" + Environment.TickCount;
    IntPtr clsPtr = Marshal.StringToHGlobalUni(cls);
    wc.lpszClassName = clsPtr;
    ushort atom = RegisterClassW(ref wc);
    if (atom == 0) { Writer.WriteLine("RegisterClassW FAILED err=" + Marshal.GetLastWin32Error()); Writer.Close(); return; }
    IntPtr hwnd = CreateWindowExW(0, clsPtr, IntPtr.Zero, 0, 0, 0, 0, 0, new IntPtr(-3), IntPtr.Zero, instance, IntPtr.Zero);
    if (hwnd == IntPtr.Zero) { Writer.WriteLine("CreateWindowExW FAILED err=" + Marshal.GetLastWin32Error()); Writer.Close(); return; }
    RAWINPUTDEVICE[] rid = new RAWINPUTDEVICE[2];
    rid[0].usUsagePage = 1;
    rid[0].usUsage = 6;
    rid[0].dwFlags = 0x00000100;
    rid[0].hwndTarget = hwnd;
    rid[1].usUsagePage = 0x0C;
    rid[1].usUsage = 1;
    rid[1].dwFlags = 0x00000100;
    rid[1].hwndTarget = hwnd;
    bool ok = RegisterRawInputDevices(rid, 2, (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICE)));
    if (!ok) { Writer.WriteLine("RegisterRawInputDevices FAILED err=" + Marshal.GetLastWin32Error()); Writer.Close(); return; }
    Writer.WriteLine("listening... hwnd=" + hwnd + " press remote buttons now");
    Writer.Flush();
    DateTime deadline = DateTime.UtcNow.AddSeconds(seconds);
    MSG msg;
    long pumped = 0;
    while (DateTime.UtcNow < deadline) {
      while (PeekMessageW(out msg, IntPtr.Zero, 0, 0, 1)) {
        if (msg.message == 0x0012) goto done;
        pumped++;
        DispatchMessageW(ref msg);
      }
      Thread.Sleep(20);
    }
    done:
    Writer.WriteLine("--- capture end " + DateTime.Now.ToString("HH:mm:ss") + " pumped=" + pumped + " ---");
    Writer.Close();
  }
}
'@
[RawKeyCap]::Run($OutFile, $Seconds)
Write-Host "capture finished -> $OutFile"
