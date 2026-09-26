"""Raw Input 后台监听（RIDEV_INPUTSINK）+ 设备归属判定。

用途
----
给写入类探针提供"Windows 输入层"的独立观测点：探针在宿主内改写了报告之后，
必须能在**被翻译之后的**层看到（或看不到）按键，才能证明改写/擦除真的生效。
没有这一层，"写入成功了"只是探针的自述。

为什么可以后台收：`RIDEV_INPUTSINK` 让注册窗口**不需要前台**也能收到 `WM_INPUT`。
本机实测该模式配 `HWND_MESSAGE`（消息专用窗口、无界面）工作正常。

两个已踩过的坑（沿用 `raw-input-dump.py` 的解法，勿改）
-------------------------------------------------------
1. **不做 `GetRawInputData` 尺寸查询**：查询调用的成功返回值是 0，与错误返回值
   容易混淆，实测会造成静默读到全零报文。这里用固定大小缓冲绕开。
2. **`WM_INPUT` 的 `HRAWINPUT` 在 `lParam`**（本机实测 `wParam` 恒为 1；用
   `wParam` 会得到 `ERROR_INVALID_HANDLE=6`），与部分文档措辞相反。

设备归属
--------
`RAWINPUTHEADER.hDevice` 可解析出设备接口路径，RC003 键盘 TLC 的路径含
`VID&012717`。因此"这个事件是不是遥控器发的"是**可判定的**，不靠猜。

线程模型
--------
窗口类注册、窗口创建、`RegisterRawInputDevices`、消息循环全部在**同一个工作线程**
内完成（Win32 要求创建窗口的线程自己泵消息）。`stop()` 用
`PostThreadMessage(tid, WM_QUIT, 0, 0)` 终止。

时间基准：所有记录带 `t = 毫秒 epoch`，与 JS 侧 `Date.now()` 同一时钟域，
可直接与 Frida 探针的记录做时间配对。
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import threading
import time

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

RIDEV_INPUTSINK = 0x00000100
RID_INPUT = 0x10000003
RIM_TYPEKEYBOARD = 1
RIM_TYPEHID = 2
WM_INPUT = 0x00FF
WM_DESTROY = 0x0002
WM_QUIT = 0x0012
HWND_MESSAGE = -3
RIDI_DEVICENAME = 0x20000007

# RC003（小米蓝牙遥控器 2 Pro）键盘 TLC 的设备接口路径特征
RC003_VID_TOKEN = "VID&012717"

# 关注的注册组合：键盘页 + 消费类（生产已注册），外加厂商页（设备声明但从未注册）
DEFAULT_REGISTRATIONS = [
    (0x01, 0x06, "Generic Desktop / Keyboard"),
    (0x0C, 0x01, "Consumer Control"),
    (0xFF00, 0x0001, "Vendor Defined 0xFF00 / 0x0001"),
    (0xFF00, 0x0002, "Vendor Defined 0xFF00 / 0x0002"),
    (0x01, 0x0080, "Generic Desktop / 0x80 System Control"),
]


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


class RAWINPUT(ctypes.Structure):
    """固定大小缓冲（header + 512B 报文），刻意不做尺寸查询。"""

    _fields_ = [
        ("header", RAWINPUTHEADER),
        ("data", ctypes.c_ubyte * 512),
    ]


WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_longlong, wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM)


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
user32.RegisterRawInputDevices.restype = wt.BOOL
user32.GetRawInputData.argtypes = [
    wt.HANDLE, wt.UINT, ctypes.c_void_p, ctypes.POINTER(wt.UINT), wt.UINT,
]
user32.GetRawInputData.restype = wt.UINT
user32.GetRawInputDeviceInfoW.argtypes = [
    wt.HANDLE, wt.UINT, ctypes.c_void_p, ctypes.POINTER(wt.UINT),
]
user32.GetRawInputDeviceInfoW.restype = wt.UINT
user32.CreateWindowExW.restype = wt.HWND
user32.DefWindowProcW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.DefWindowProcW.restype = ctypes.c_longlong


def device_name(handle) -> str:
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


class RawInputWatcher:
    """后台 Raw Input 监听。事件以 dict 记录，带毫秒 epoch 时间戳与设备归属。

    每个事件字段：
        t       毫秒 epoch（与 Frida `Date.now()` 同源）
        kind    'kb' | 'hid' | 'other'
        vk      仅 kb：虚拟键码（VKey）
        make   仅 kb：扫描码（MakeCode）
        flags  仅 kb：RAWKEYBOARD.Flags（bit0=BREAK，bit1=E0，bit2=E1）
        msg    仅 kb：0x0100 按下 / 0x0101 抬起
        size/count/payload  仅 hid
        device  设备接口路径
        rc003   是否该设备为 RC003
    """

    def __init__(self, on_event=None, registrations=None) -> None:
        self.events: list[dict] = []
        self.on_event = on_event
        self._registrations = registrations or DEFAULT_REGISTRATIONS
        self._thread: threading.Thread | None = None
        self._tid: int = 0
        self._ready = threading.Event()
        self._stopped = threading.Event()
        self.registration_ok: list[tuple[int, int, str, bool, int]] = []
        self.error: str | None = None
        self._wndproc_ref = None  # 防 GC

    # ---------------- 生命周期 ----------------

    def start(self, timeout: float = 8.0) -> bool:
        self._thread = threading.Thread(target=self._run, name="raw-input-sink", daemon=True)
        self._thread.start()
        return self._ready.wait(timeout) and self.error is None

    def stop(self, timeout: float = 5.0) -> None:
        if self._thread is None:
            return
        if self._tid:
            user32.PostThreadMessageW(wt.DWORD(self._tid), WM_QUIT, 0, 0)
        self._thread.join(timeout)
        self._stopped.set()

    # ---------------- 线程体 ----------------

    def _run(self) -> None:
        try:
            self._tid = kernel32.GetCurrentThreadId()
            instance = kernel32.GetModuleHandleW(None)
            class_name = f"SayAllRawSinkWnd{int(time.time() * 1000) % 100000000}"

            def proc(hwnd, message, wparam, lparam):
                if message == WM_INPUT:
                    self._handle_input(wt.HANDLE(lparam & 0xFFFFFFFFFFFFFFFF))
                    return 0
                if message == WM_DESTROY:
                    user32.PostQuitMessage(0)
                    return 0
                return user32.DefWindowProcW(hwnd, message, wparam, lparam)

            self._wndproc_ref = WNDPROC(proc)
            wnd_class = WNDCLASSW()
            wnd_class.lpfnWndProc = self._wndproc_ref
            wnd_class.hInstance = instance
            wnd_class.lpszClassName = class_name
            if user32.RegisterClassW(ctypes.byref(wnd_class)) == 0:
                self.error = f"RegisterClassW err={ctypes.get_last_error()}"
                return
            hwnd = user32.CreateWindowExW(
                0, class_name, "raw-sink", 0, 0, 0, 0, 0,
                wt.HWND(HWND_MESSAGE), None, instance, None,
            )
            if not hwnd:
                self.error = f"CreateWindowExW err={ctypes.get_last_error()}"
                return

            for page, usage, note in self._registrations:
                device = RAWINPUTDEVICE(page, usage, RIDEV_INPUTSINK, hwnd)
                ok = bool(
                    user32.RegisterRawInputDevices(
                        ctypes.byref(device), 1, ctypes.sizeof(RAWINPUTDEVICE)
                    )
                )
                self.registration_ok.append(
                    (page, usage, note, ok, ctypes.get_last_error() if not ok else 0)
                )

            self._ready.set()

            message = wt.MSG()
            while True:
                result = user32.GetMessageW(ctypes.byref(message), None, 0, 0)
                if result in (0, -1):
                    break
                if message.message == WM_QUIT:
                    break
                user32.TranslateMessage(ctypes.byref(message))
                user32.DispatchMessageW(ctypes.byref(message))
            user32.DestroyWindow(hwnd)
        except Exception as exc:  # noqa: BLE001 - 现场需要看到原始错误
            self.error = f"{type(exc).__name__}: {exc}"
        finally:
            self._ready.set()

    def _handle_input(self, handle) -> None:
        header_size = ctypes.sizeof(RAWINPUTHEADER)
        raw = RAWINPUT()
        size = wt.UINT(ctypes.sizeof(RAWINPUT))
        written = user32.GetRawInputData(
            handle, RID_INPUT, ctypes.byref(raw), ctypes.byref(size), header_size
        )
        if written == 0xFFFFFFFF:
            return
        header = raw.header
        body = ctypes.addressof(raw) + header_size
        path = device_name(header.hDevice)
        record = {
            "t": time.time() * 1000.0,
            "device": path,
            "rc003": RC003_VID_TOKEN in path,
        }

        if header.dwType == RIM_TYPEKEYBOARD:
            kb = ctypes.cast(body, ctypes.POINTER(RAWKEYBOARD)).contents
            record.update(
                kind="kb",
                vk=int(kb.VKey),
                make=int(kb.MakeCode),
                flags=int(kb.Flags),
                msg=int(kb.Message),
            )
        elif header.dwType == RIM_TYPEHID:
            size_hid = ctypes.c_uint32.from_address(body).value
            count = ctypes.c_uint32.from_address(body + 4).value
            payload = bytes(
                ctypes.string_at(body + 8, min(size_hid * count, 512))
            )
            record.update(kind="hid", size=size_hid, count=count, payload=payload)
        else:
            record.update(kind="other", raw_type=int(header.dwType))

        self.events.append(record)
        if self.on_event is not None:
            self.on_event(record)


class MOUSEINPUT(ctypes.Structure):
    """只为了让 INPUT 联合体尺寸正确（x64 下 sizeof(INPUT)=40）。"""

    _fields_ = [
        ("dx", wt.LONG),
        ("dy", wt.LONG),
        ("mouseData", wt.DWORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", ctypes.c_ushort),
        ("wScan", ctypes.c_ushort),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]


class INPUT(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", wt.DWORD), ("u", _INPUTUNION)]


INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002

user32.SendInput.argtypes = [wt.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
user32.SendInput.restype = wt.UINT


def send_key(vk: int) -> bool:
    """用 SendInput 注入一次按下+抬起（本探针唯一的输入注入，仅用于自检）。

    默认注入 F13：本机没有任何应用会响应它，因此**零可见副作用**。
    """
    events = (INPUT * 2)()
    for index, flags in ((0, 0), (1, KEYEVENTF_KEYUP)):
        events[index].type = INPUT_KEYBOARD
        events[index].ki = KEYBDINPUT(
            wVk=vk, wScan=0, dwFlags=flags, time=0, dwExtraInfo=None
        )
    return user32.SendInput(2, events, ctypes.sizeof(INPUT)) == 2


def self_test(vk: int = 0x7C, manual_seconds: float = 0.0) -> int:
    """观测通道自检。

    阳性对照：`SendInput` 注入一个已知键，监听必须读到它。没有这一步，
    采集阶段的"零事件"无法区分"设备没发"与"监听坏了"。

    `manual_seconds > 0` 时再开一个手动窗口，用于确认**真实设备**可被观测
    （正式采集前建议先按一次 确定 试试）。
    """
    watcher = RawInputWatcher()
    if not watcher.start():
        print("监听启动失败:", watcher.error)
        return 1
    for page, usage, note, ok, err in watcher.registration_ok:
        print(f"  注册 {'成功' if ok else f'失败 err={err}'} "
              f"page=0x{page:04X} usage=0x{usage:04X}  {note}")

    print(f"\n[阳性对照] SendInput 注入 vk=0x{vk:02X}（按下+抬起）……")
    time.sleep(0.5)
    injected_ok = send_key(vk)
    time.sleep(1.0)
    hits = [e for e in watcher.events if e["kind"] == "kb" and e["vk"] == vk]
    for event in hits:
        print(f"  vk=0x{event['vk']:02X} make=0x{event['make']:02X} "
              f"msg=0x{event['msg']:04X} rc003={event['rc003']}")
    print(f"  SendInput 调用成功: {injected_ok}；监听捕获到 {len(hits)} 条")
    control_ok = injected_ok and len(hits) >= 1

    rc003_seen = 0
    if manual_seconds > 0:
        print(f"\n[真实设备] {manual_seconds:.0f} 秒内请按遥控器的 确定 键……")
        before = len(watcher.events)
        time.sleep(manual_seconds)
        for event in watcher.events[before:]:
            if event["kind"] == "kb" and event["rc003"]:
                rc003_seen += 1
                print(f"  vk=0x{event['vk']:02X} make=0x{event['make']:02X} "
                      f"msg=0x{event['msg']:04X} rc003=True  <<< RC003")
        print(f"  捕获 RC003 事件 {rc003_seen} 条")

    watcher.stop()
    print("\n=== 观测通道自检 ===")
    print(f"  合成输入阳性对照: {'通过' if control_ok else '失败'}")
    if manual_seconds > 0:
        print(f"  真实设备可观测: {'通过' if rc003_seen else '未捕获到（按了 确定 吗？）'}")
    return 0 if control_ok else 2


if __name__ == "__main__":
    import argparse

    cli = argparse.ArgumentParser(description="Raw Input 后台监听自检")
    cli.add_argument("--vk", type=lambda s: int(s, 0), default=0x7C,
                     help="SendInput 注入的虚拟键码，默认 0x7C(F13)，零可见副作用")
    cli.add_argument("--manual", type=float, default=0.0,
                     help="额外的真机手动窗口秒数（按遥控器 确定）")
    args = cli.parse_args()
    raise SystemExit(self_test(args.vk, args.manual))
