"""豆包"免提模式"实验：三、注入可达性判定（辅助探针，非判定依据）

目的：把"注入是否到达系统输入流"这一层与"豆包是否响应"分开，
      避免把"豆包没响应"直接归因为"豆包过滤注入"。

回答的问题：**我自己注入的右 Alt，在系统 LL 键盘钩子链上是否可见、
              以及是否带 LLKHF_INJECTED 标志。**

⚠️ 重要局限（别过度解读）：
LL 钩子是"链式"的——每个钩子收到**私有副本**，修改不跨钩子传播。
本脚本只能观测"我自己的钩子看到了什么"，**不能**据此断定豆包做了什么
（豆包有自己的钩子副本，它看到什么我们看不到）。
因此本脚本**只输出事实，不下结论**；A/B/C 判定仍须物理键对照
（见 `2-probe-injection-vs-physical.py --watch`）。

用法（**必须加 `-u`**，否则 Python 缓冲会让输出丢失）：
  python -u 3-probe-injection-reachability.py --inject
      装 LL 钩子 → 注入右 Alt 点击 → 报告自己的钩子收到几个事件、
      其中几个带 LLKHF_INJECTED 标志。
  python -u 3-probe-injection-reachability.py --inject --physical-note
      注入前后各留窗口请真人按一次物理右 Alt，做**同一次运行内**的对照
      （物理事件 LLKHF_INJECTED 必须为 0）。

实现约束（踩过的坑，别改回去）：
1. **钩子回调必须极快返回**。回调里做 `time.sleep`、复杂解析或抛异常，
   都会让 Windows 判定钩子超时并**静默摘钩**，表现为脚本"跑到一半就不动了"。
   本实现只在回调里存一条最小记录，其余处理全部放到消息泵之外。
2. **回调对象必须保持强引用**（`_hook_ref`），否则被 GC 回收后系统回调
   野指针，进程直接死掉。
3. `CallNextHookEx` 的 hook 句柄传自身句柄（文档允许传 NULL，但传自身更稳）。
"""

import argparse
import ctypes
import time
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)

WH_KEYBOARD_LL = 13
HC_ACTION = 0

WM_KEYDOWN = 0x0100
WM_KEYUP = 0x0101
WM_SYSKEYDOWN = 0x0104
WM_SYSKEYUP = 0x0105

LLKHF_INJECTED = 0x10
LLKHF_LOWER_IL_INJECTED = 0x02
LLKHF_EXTENDED = 0x01

VK_RMENU = 0xA5
RIGHT_ALT_SCAN = 0x38

INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_EXTENDEDKEY = 0x0001
KEYEVENTF_SCANCODE = 0x0008

# 关心的虚拟键：右 Alt 与通用 Alt
WATCHED_VKS = {VK_RMENU, 0x12}


class KBDLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [
        ("vkCode", wintypes.DWORD),
        ("scanCode", wintypes.DWORD),
        ("flags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_void_p),
    ]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD),
                ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_void_p)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wintypes.LONG), ("dy", wintypes.LONG),
                ("mouseData", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_void_p)]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wintypes.DWORD), ("u", _INPUTUNION)]


assert ctypes.sizeof(INPUT) == 40, (
    f"INPUT 大小 {ctypes.sizeof(INPUT)} != 40，注入会静默失效"
)

HOOKPROC = ctypes.WINFUNCTYPE(
    ctypes.c_ssize_t, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM
)

user32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC,
                                     wintypes.HINSTANCE, wintypes.DWORD]
user32.SetWindowsHookExW.restype = wintypes.HHOOK
user32.CallNextHookEx.argtypes = [wintypes.HHOOK, ctypes.c_int,
                                  wintypes.WPARAM, wintypes.LPARAM]
user32.CallNextHookEx.restype = ctypes.c_ssize_t
user32.UnhookWindowsHookEx.argtypes = [wintypes.HHOOK]
user32.UnhookWindowsHookEx.restype = wintypes.BOOL

user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
user32.SendInput.restype = wintypes.UINT

# 钩子回调里只做这一件事：追加一条裸元组。保持回调极短。
_raw_events = []
_hook_handle = None
# 强引用：回调对象被 GC 会令系统回调野指针 → 进程崩溃。
_hook_ref = None


def _hook_proc(code, wparam, lparam):
    try:
        if code == HC_ACTION:
            info = ctypes.cast(lparam, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
            if info.vkCode in WATCHED_VKS:
                _raw_events.append(
                    (int(wparam), int(info.vkCode), int(info.scanCode), int(info.flags))
                )
    except Exception:
        # 回调里绝不抛出：异常会顺着系统回调边界走，行为未定义。
        pass
    return user32.CallNextHookEx(_hook_handle, code, wparam, lparam)


def _install_hook() -> bool:
    global _hook_handle, _hook_ref
    _hook_ref = HOOKPROC(_hook_proc)
    _hook_handle = user32.SetWindowsHookExW(WH_KEYBOARD_LL, _hook_ref, None, 0)
    return bool(_hook_handle)


def _uninstall_hook() -> None:
    global _hook_handle
    if _hook_handle:
        user32.UnhookWindowsHookEx(_hook_handle)
        _hook_handle = None


def _key_event(scan: int, flags: int) -> INPUT:
    item = INPUT()
    item.type = INPUT_KEYBOARD
    item.u.ki.wVk = 0
    item.u.ki.wScan = scan
    item.u.ki.dwFlags = flags
    item.u.ki.dwExtraInfo = None
    return item


def send_one(scan: int, flags: int) -> bool:
    """逐事件提交，贴近物理时序。"""
    item = _key_event(scan, flags)
    return user32.SendInput(1, ctypes.byref(item), ctypes.sizeof(INPUT)) == 1


def right_alt_tap(gap_seconds: float = 0.12) -> bool:
    ok_down = send_one(RIGHT_ALT_SCAN,
                       KEYEVENTF_EXTENDEDKEY | KEYEVENTF_SCANCODE)
    time.sleep(gap_seconds)
    ok_up = send_one(RIGHT_ALT_SCAN,
                     KEYEVENTF_EXTENDEDKEY | KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP)
    return ok_down and ok_up


def pump(seconds: float) -> int:
    """跑消息循环让钩子回调有机会被调用。返回处理的消息数。

    ⚠️ 必须周期性 `PeekMessageW`：LL 钩子回调是**在安装它的线程上**被调用的，
    该线程必须持续取消息，否则 Windows 会让钩子超时。
    """
    msg = wintypes.MSG()
    handled = 0
    deadline = time.time() + seconds
    while time.time() < deadline:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))
            handled += 1
        time.sleep(0.002)
    return handled


_MSG_NAMES = {
    WM_KEYDOWN: "DOWN",
    WM_KEYUP: "UP",
    WM_SYSKEYDOWN: "SYSDOWN",
    WM_SYSKEYUP: "SYSUP",
}


def report(label: str) -> None:
    print(f"\n--- {label} ---", flush=True)
    if not _raw_events:
        print("  (未捕获任何 Alt 事件)", flush=True)
        return
    for msg, vk, scan, flags in _raw_events:
        name = _MSG_NAMES.get(msg, hex(msg))
        print(
            f"  vk=0x{vk:02X} scan=0x{scan:02X} {name:<8} "
            f"injected={bool(flags & LLKHF_INJECTED)} "
            f"lower_il={bool(flags & LLKHF_LOWER_IL_INJECTED)} "
            f"extended={bool(flags & LLKHF_EXTENDED)}",
            flush=True,
        )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--inject", action="store_true", help="注入右 Alt 点击")
    ap.add_argument("--pump-seconds", type=float, default=1.5)
    ap.add_argument("--physical-note", action="store_true",
                    help="注入前先留 6 秒请真人按一次物理右 Alt 做同运行对照")
    ap.add_argument("--physical-window", type=float, default=6.0)
    args = ap.parse_args()

    if not _install_hook():
        print(f"!! 装钩子失败 LastError={ctypes.get_last_error()}", flush=True)
        return 2
    print(f"LL 键盘钩子已装载 hwnd={_hook_handle}", flush=True)

    try:
        if args.physical_note:
            print(f"\n>>> 现在请真人按一次【物理键盘右 Alt】"
                  f"（{args.physical_window:.0f} 秒内），用于与注入对照…", flush=True)
            pump(args.physical_window)
            report("真人物理按键（LLKHF_INJECTED 应为 false）")
            _raw_events.clear()

        if args.inject:
            print("\n>>> 注入右 Alt 点击…", flush=True)
            ok = right_alt_tap()
            print(f"    注入调用返回 ok={ok}", flush=True)
            pump(args.pump_seconds)
            report("注入按键捕获")
    finally:
        _uninstall_hook()
        print("\n钩子已卸载", flush=True)

    print(
        "\n说明：本脚本只报告『我自己的 LL 钩子看到什么』。LL 钩子是链式的，"
        "每个钩子收到私有副本，因此这些数据**不能**用来推断豆包的行为——"
        "A/B/C 判定仍需物理键对照（见 2-probe-... --watch）。",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
