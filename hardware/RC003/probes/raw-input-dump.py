"""RC003 免驱动通道探针 B：Raw Input 全量事件转储。

目的
----
2026-09-22 E1 的结论是「该设备 Raw Input 的 HID 通道零报文、键盘通道有事件」，
但 E1 的生产监听只注册了两个 TLC：

    usUsagePage=0x01 usUsage=0x06   (Generic Desktop / Keyboard)
    usUsagePage=0x0C usUsage=0x01   (Consumer Control)

而 2026-09-23 的报告描述符取证（见 hid-direct-control.log）显示该设备的**唯一**
TLC 是键盘（UP 0x01 / U 0x06），并在其中额外声明了厂商页输入报告：

    page 0x0007 report_id 0x01  usage 0x0000-0x00FE   <- 三键 0x80/0x81/0xF1 在这里
    page 0xFF00 report_id 0x06  usage 0x0000-0x00FF
    page 0xFF00 report_id 0x07  usage 0x0000-0x00FF
    page 0xFF00 report_id 0x08  usage 0x0000-0x00FF

本探针把**所有可能相关的注册组合**一次性打开，并对该设备的**每一条** WM_INPUT
原样记录（不做任何 VK 过滤），回答：

 1. 按【返回/音量±】时，设备是否产生**任何** Raw Input 事件？
    - 若键盘通道出现 VKey=0xFF + 特异 MakeCode，则三键实际可达（"直接归因族"），
      产品侧已启用的映射即可生效。
    - 若完全没有事件，则 kbdhid 在映射层丢弃（usage 无对应 VK），免驱动无解。
 2. 厂商页注册（0xFF00/0x0001）能否匹配到该设备？RIM 是按 TLC 的
    usage page/usage 匹配注册项的，该设备 TLC 是 0x01/0x06，因此预期不匹配——
    本探针把这个预期变成可核验的事实。
 3. 该设备是否产生 RIM_TYPEHID（type=2）报文。

安全性：纯只读观测。不做钩子、不吞键、不注入、不改注册表。

用法：
  python raw-input-dump.py --seconds 120 [--out 文件]

窗口用 HWND_MESSAGE（消息专用窗口），不显示任何界面。
"""
import ctypes
import ctypes.wintypes as wt
import sys
import time

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

# ---------- 常量 ----------

RIDEV_INPUTSINK = 0x00000100
RID_INPUT = 0x10000003
RIM_TYPEKEYBOARD = 1
RIM_TYPEHID = 2
WM_INPUT = 0x00FF
WM_DESTROY = 0x0002
HWND_MESSAGE = -3
VK_LEFT = 0x25

# 关注的注册组合：生产监听的两个 + 设备声明但从未注册的厂商页
REGISTRATIONS = [
    (0x01, 0x06, "Generic Desktop / Keyboard（生产已注册）"),
    (0x0C, 0x01, "Consumer Control（生产已注册）"),
    (0xFF00, 0x0001, "Vendor Defined 0xFF00 / 0x0001（从未注册）"),
    (0xFF00, 0x0002, "Vendor Defined 0xFF00 / 0x0002（从未注册）"),
    (0x01, 0x0080, "Generic Desktop / 0x80 System Control（从未注册）"),
]

_hid_pages = {
    0x01: "Generic Desktop",
    0x07: "Keyboard",
    0x0C: "Consumer",
    0xFF00: "Vendor Defined",
}


class RAWINPUTDEVICE(ctypes.Structure):
    _fields_ = [
        ("usUsagePage", ctypes.c_ushort),
        ("usUsage", ctypes.c_ushort),
        ("dwFlags", wt.DWORD),
        ("hwndTarget", wt.HWND),
    ]


class RAWINPUTHEADER(ctypes.Structure):
    _fields_ = [
        ("dwType", wt.DWORD),
        ("dwSize", wt.DWORD),
        ("hDevice", wt.HANDLE),
        ("wParam", wt.WPARAM),
    ]


class RAWKEYBOARD(ctypes.Structure):
    _fields_ = [
        ("MakeCode", ctypes.c_ushort),
        ("Flags", ctypes.c_ushort),
        ("Reserved", ctypes.c_ushort),
        ("VKey", ctypes.c_ushort),
        ("Message", wt.UINT),
        ("ExtraInformation", ctypes.c_ulong),
    ]


class RAWHID(ctypes.Structure):
    _fields_ = [
        ("dwSizeHid", wt.DWORD),
        ("dwCount", wt.DWORD),
        ("bRawData", ctypes.c_ubyte * 1),
    ]


class RAWINPUT(ctypes.Structure):
    """固定大小缓冲：RAWINPUTHEADER + 最大报文体。

    刻意**不做** GetRawInputData 的尺寸查询——查询调用的成功返回值是 0，
    与错误返回值容易混淆，实测会造成静默读到全零报文。固定缓冲绕开这个坑。
    """

    _fields_ = [
        ("header", RAWINPUTHEADER),
        ("data", ctypes.c_ubyte * 512),
    ]


WNDPROC = ctypes.WINFUNCTYPE(
    ctypes.c_longlong, wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM
)


class WNDCLASSW(ctypes.Structure):
    _fields_ = [
        ("style", wt.UINT),
        ("lpfnWndProc", WNDPROC),
        ("cbClsExtra", ctypes.c_int),
        ("cbWndExtra", ctypes.c_int),
        ("hInstance", wt.HINSTANCE),
        ("hIcon", wt.HICON),
        ("hCursor", wt.HANDLE),
        ("hbrBackground", wt.HBRUSH),
        ("lpszMenuName", wt.LPCWSTR),
        ("lpszClassName", wt.LPCWSTR),
    ]


user32.RegisterRawInputDevices.argtypes = [
    ctypes.POINTER(RAWINPUTDEVICE), wt.UINT, wt.UINT,
]
user32.GetRawInputData.argtypes = [
    wt.HANDLE, wt.UINT, ctypes.c_void_p, ctypes.POINTER(wt.UINT), wt.UINT,
]
user32.GetRawInputData.restype = wt.UINT
user32.GetRawInputDeviceInfoW.restype = wt.UINT
user32.RegisterRawInputDevices.restype = wt.BOOL
user32.GetRawInputDeviceInfoW.argtypes = [
    wt.HANDLE, wt.UINT, ctypes.c_void_p, ctypes.POINTER(wt.UINT),
]
user32.CreateWindowExW.restype = wt.HWND
user32.DefWindowProcW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.DefWindowProcW.restype = ctypes.c_longlong

RIDI_DEVICENAME = 0x20000007

_out = []


def log(line=""):
    text = str(line)
    _out.append(text)
    print(text, flush=True)


def mask(text):
    import re
    return re.sub(r"[0-9a-fA-F]{12}", "<BT-ADDR>", text)


def device_name(handle):
    size = wt.UINT(0)
    user32.GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, None, ctypes.byref(size))
    if size.value == 0:
        return "?"
    buffer = ctypes.create_unicode_buffer(size.value + 1)
    written = user32.GetRawInputDeviceInfoW(
        handle, RIDI_DEVICENAME, buffer, ctypes.byref(size)
    )
    if written in (0xFFFFFFFF, 0):
        return "?"
    return buffer.value


def hexs(data, limit=32):
    return " ".join(f"{b:02X}" for b in data[:limit])


def handle_input(handle):
    header_size = ctypes.sizeof(RAWINPUTHEADER)
    raw = RAWINPUT()
    size = wt.UINT(ctypes.sizeof(RAWINPUT))
    written = user32.GetRawInputData(
        handle, RID_INPUT, ctypes.byref(raw), ctypes.byref(size), header_size
    )
    if written == 0xFFFFFFFF:
        log(f"[?] GetRawInputData 失败 err={ctypes.get_last_error()}")
        return
    header = raw.header
    body = ctypes.addressof(raw) + header_size
    path = device_name(header.hDevice)
    remote = "VID&012717" in path

    if header.dwType == RIM_TYPEKEYBOARD:
        kb = ctypes.cast(body, ctypes.POINTER(RAWKEYBOARD)).contents
        flag = " <<< RC003" if remote else ""
        log(f"[RAW-KEYBOARD]{flag} vk=0x{kb.VKey:02X} make=0x{kb.MakeCode:02X} "
            f"flags=0x{kb.Flags:04X} msg=0x{kb.Message:04X}")
    elif header.dwType == RIM_TYPEHID:
        # RAWHID: dwSizeHid, dwCount, bRawData[]
        size_hid = ctypes.c_uint32.from_address(body).value
        count = ctypes.c_uint32.from_address(body + 4).value
        payload = bytes(ctypes.string_at(body + 8, min(size_hid * count, 512)))
        flag = " <<< RC003" if remote else ""
        log(f"[RAW-HID]{flag} size={size_hid} count={count} b=[{hexs(payload)}]")
    else:
        blob = bytes(ctypes.string_at(ctypes.addressof(raw), min(written, 48)))
        log(f"[RAW-OTHER] type={header.dwType} dwSize={header.dwSize} "
            f"written={written} hex=[{hexs(blob)}]")


def build_wnd_proc():
    def proc(hwnd, message, wparam, lparam):
        if message == WM_INPUT:
            # 实测（本机 Windows，RIDEV_INPUTSINK + 消息专用窗口）：
            # WM_INPUT 的 wParam 恒为 1，HRAWINPUT 在 **lParam** 里。
            # 这与部分文档措辞相反，但与 GetRawInputData 的实际行为一致
            # （用 wParam 会返回 ERROR_INVALID_HANDLE=6）。
            handle_input(wt.HANDLE(lparam & 0xFFFFFFFFFFFFFFFF))
            return 0
        if message == WM_DESTROY:
            user32.PostQuitMessage(0)
            return 0
        return user32.DefWindowProcW(hwnd, message, wparam, lparam)

    return WNDPROC(proc)


def main():
    seconds = 120
    out_path = None
    index = 1
    while index < len(sys.argv):
        if sys.argv[index] == "--seconds" and index + 1 < len(sys.argv):
            seconds = int(sys.argv[index + 1])
            index += 2
        elif sys.argv[index] == "--out" and index + 1 < len(sys.argv):
            out_path = sys.argv[index + 1]
            index += 2
        else:
            index += 1

    log(f"=== raw-input-dump 开始 {time.strftime('%Y-%m-%dT%H:%M:%S')} ===")

    keep_alive = build_wnd_proc()
    instance = kernel32.GetModuleHandleW(None)
    class_name = "SayAllRawDumpWnd"
    wnd_class = WNDCLASSW()
    wnd_class.lpfnWndProc = keep_alive
    wnd_class.hInstance = instance
    wnd_class.lpszClassName = class_name
    atom = user32.RegisterClassW(ctypes.byref(wnd_class))
    if atom == 0:
        log(f"RegisterClassW 失败 err={ctypes.get_last_error()}")
        return
    hwnd = user32.CreateWindowExW(
        0, class_name, "raw-dump", 0, 0, 0, 0, 0, wt.HWND(HWND_MESSAGE), None, instance, None
    )
    if not hwnd:
        log(f"CreateWindowExW 失败 err={ctypes.get_last_error()}")
        return

    log("\n[1] 逐项注册 Raw Input（RIDEV_INPUTSINK）：")
    for page, usage, note in REGISTRATIONS:
        device = RAWINPUTDEVICE(page, usage, RIDEV_INPUTSINK, hwnd)
        ok = user32.RegisterRawInputDevices(
            ctypes.byref(device), 1, ctypes.sizeof(RAWINPUTDEVICE)
        )
        if ok:
            log(f"    注册成功 page=0x{page:04X} usage=0x{usage:04X}  {note}")
        else:
            log(f"    注册失败 page=0x{page:04X} usage=0x{usage:04X} "
                f"err={ctypes.get_last_error()}  {note}")

    user32.SetTimer(hwnd, 1, seconds * 1000, None)

    log(f"\n[2] 开始采集 {seconds} 秒。")
    log("    请按：返回 ×1 →（停 3 秒）音量+ ×2 →（停 3 秒）音量- ×3")
    log("         →（停 3 秒）确定 ×2 →（停 3 秒）主页 ×1（已知可用的对照组）")
    log("    注意：本探针不做任何 VK 过滤，遥控器的**每一条**事件都会被打印。")

    message = wt.MSG()
    start = time.time()
    while True:
        result = user32.GetMessageW(ctypes.byref(message), None, 0, 0)
        if result in (0, -1):
            break
        if message.message == 0x0113:  # WM_TIMER
            break
        user32.TranslateMessage(ctypes.byref(message))
        user32.DispatchMessageW(ctypes.byref(message))
        if time.time() - start > seconds + 10:
            break

    log(f"\n=== 采集结束，用时 {time.time() - start:.1f}s ===")
    user32.DestroyWindow(hwnd)

    if out_path:
        with open(out_path, "w", encoding="utf-8") as file:
            file.write("\n".join(_out) + "\n")


if __name__ == "__main__":
    main()
