"""Windows 侧独立判据：LL 钩子按键观测器（**能读回 LLKHF_INJECTED 位**）。

为什么不能用既有的 Raw Input 观测器
------------------------------------
`raw_input_sink.py` 能按 `hDevice` 判断按键来自哪台设备，但 **Raw Input 的
RAWKEYBOARD 不带"是否为合成输入"的标志**。而本轮实验要回答的问题恰恰是：

    这份由我们在报告层合成出来的按键，到达 Windows 输入流时，
    到底带不带 `LLKHF_INJECTED`（0x10）？

豆包的 `ImeService` 全局 LL 钩子回调首查的就是这一位（见
`Bugs/2026-09-04-doubao-voice-hold-hotkey.md` 四层闭环）。因此**判据必须在
LL 钩子层读数**，而不是在 Raw Input 层——后者无论我们用哪种方式产键，
读到的形态都一样，没有任何分辨力。

`LLKHF_INJECTED` 是什么（给不熟悉 Windows 的读者）
-------------------------------------------------
Windows 给"程序模拟出来的按键"打的标记位。凡是 `SendInput` / `keybd_event` /
InputInjector 产生的按键，钩子回调收到的 `flags` 里都会带它，应用程序据此可以
区分"真人按的"和"程序发的"。物理键盘产生的按键不带。

flags 各位（KBDLLHOOKSTRUCT.flags）
-----------------------------------
    0x01 LLKHF_EXTENDED           扩展键
    0x02 LLKHF_LOWER_IL_INJECTED  来自更低完整性级别的进程
    0x10 LLKHF_INJECTED           来自合成输入（**本实验唯一关心的位**）
    0x20 LLKHF_ALTDOWN            同时按着 Alt
    0x80 LLKHF_UP                 这是释放沿

观测器的自检要有**双向分辨力**
------------------------------
只有"能读到 injected=1"不算自检通过——那可能只是个恒真的读数。自检必须证明：

    1. SendInput 注入 F13   -> 读到 injected=1   （知道怎么判"合成"）
    2. 用户按下物理 F13/某键 -> 读到 injected=0   （知道怎么判"真实"）

两个方向都对，才谈得上用它去判第三种来源（我们的合成 HID 报告）。

不需要提权：安装 LL 钩子只要求本进程有自己的消息泵，与完整性级别无关。

脱敏：只记录虚拟键码与标志位，不记录窗口标题、进程名或任何文本内容。
"""

from __future__ import annotations

import ctypes
import threading
import time
from ctypes import wintypes as wt

WH_KEYBOARD_LL = 13
HC_ACTION = 0
WM_QUIT = 0x0012

LLKHF_EXTENDED = 0x0001
LLKHF_LOWER_IL_INJECTED = 0x0002
LLKHF_INJECTED = 0x0010
LLKHF_ALTDOWN = 0x0020
LLKHF_UP = 0x0080

INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

LRESULT = ctypes.c_ssize_t
HOOKPROC = ctypes.WINFUNCTYPE(LRESULT, ctypes.c_int, wt.WPARAM, wt.LPARAM)


class KBDLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [
        ("vkCode", wt.DWORD),
        ("scanCode", wt.DWORD),
        ("flags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.c_ulonglong),
    ]


class POINT(ctypes.Structure):
    _fields_ = [("x", wt.LONG), ("y", wt.LONG)]


class MSG(ctypes.Structure):
    _fields_ = [
        ("hWnd", wt.HWND),
        ("message", wt.UINT),
        ("wParam", wt.WPARAM),
        ("lParam", wt.LPARAM),
        ("time", wt.DWORD),
        ("pt", POINT),
        ("lPrivate", wt.DWORD),
    ]


class MOUSEINPUT(ctypes.Structure):
    """只为让 INPUT 联合体在 x64 下尺寸正确（sizeof(INPUT)==40）。"""

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


user32.SetWindowsHookExW.restype = wt.HHOOK
user32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, wt.HINSTANCE, wt.DWORD]
# 64 位陷阱：不设 restype 时句柄按 32 位 c_int 返回，高位被截断，
# 传给 SetWindowsHookExW 就是无效模块句柄（err=126 ERROR_MOD_NOT_FOUND）。
kernel32.GetModuleHandleW.restype = wt.HMODULE
kernel32.GetModuleHandleW.argtypes = [wt.LPCWSTR]
user32.UnhookWindowsHookEx.argtypes = [wt.HHOOK]
user32.UnhookWindowsHookEx.restype = wt.BOOL
user32.CallNextHookEx.restype = LRESULT
user32.CallNextHookEx.argtypes = [wt.HHOOK, ctypes.c_int, wt.WPARAM, wt.LPARAM]
user32.PostThreadMessageW.argtypes = [wt.DWORD, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.PostThreadMessageW.restype = wt.BOOL
user32.SendInput.argtypes = [wt.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
user32.SendInput.restype = wt.UINT

VK_NAMES = {
    0xA5: "VK_RMENU(右Alt)",
    0xA4: "VK_LMENU(左Alt)",
    0xA3: "VK_RCONTROL",
    0xA2: "VK_LCONTROL",
    0x5B: "VK_LWIN",
    0x5C: "VK_RWIN",
    0x0D: "VK_RETURN",
    0x24: "VK_HOME",
    0x7C: "VK_F13",
    0x0A: "VK_BACK?",
}


def vk_label(vk: int) -> str:
    return VK_NAMES.get(vk, f"0x{vk:02X}")


def send_key(vk: int, hold_ms: int = 0) -> bool:
    """SendInput 注入一次按下+抬起；`hold_ms > 0` 时在 DOWN 与 UP 之间停留。

    这是**阳性对照**用的注入器：它产生的按键必然带 LLKHF_INJECTED。
    没有它就无法区分"观测器坏了"与"真的没有事件"。
    """
    # 两个独立 INPUT 分两次发送。不要用 `byref(events, offset)` 试图发第二个
    # 元素：argtypes 声明为 POINTER(INPUT) 后，byref 数组产生的是"指向数组"
    # 的指针类型，ctypes 校验直接拒绝（expected LP_INPUT）；而且 byref 的
    # 第二参数是**字节**偏移，写 1 也根本不是元素 1。
    down = INPUT()
    down.type = INPUT_KEYBOARD
    down.ki = KEYBDINPUT(wVk=vk, wScan=0, dwFlags=0, time=0, dwExtraInfo=None)
    up = INPUT()
    up.type = INPUT_KEYBOARD
    up.ki = KEYBDINPUT(wVk=vk, wScan=0, dwFlags=KEYEVENTF_KEYUP, time=0,
                       dwExtraInfo=None)
    sent = user32.SendInput(1, ctypes.byref(down), ctypes.sizeof(INPUT)) == 1
    if hold_ms > 0:
        time.sleep(hold_ms / 1000.0)
    ok_up = user32.SendInput(1, ctypes.byref(up), ctypes.sizeof(INPUT)) == 1
    return sent and ok_up


class LLKeyWatcher:
    """在独立线程里跑消息泵并安装 WH_KEYBOARD_LL 钩子。

    接口刻意与既有的 `RawInputWatcher` 对齐（`start/stop/events/error`），
    这样上层的 driver 换观测源时改动最小。
    """

    def __init__(self, on_event=None) -> None:
        self.events: list[dict] = []
        self.error: str | None = None
        self.installed: bool = False
        self._on_event = on_event
        self._lock = threading.Lock()
        self._hook = None
        self._cb = None
        self._thread: threading.Thread | None = None
        self._thread_id: int | None = None
        self._ready = threading.Event()

    # ---------------------------------------------------------------- 生命周期

    def start(self, timeout: float = 8.0) -> bool:
        self._thread = threading.Thread(target=self._run, daemon=True,
                                        name="ll-hook-pump")
        self._thread.start()
        return self._ready.wait(timeout) and self.error is None and self.installed

    def stop(self, timeout: float = 5.0) -> None:
        if self._thread_id is not None:
            user32.PostThreadMessageW(self._thread_id, WM_QUIT, 0, 0)
        if self._thread is not None:
            self._thread.join(timeout)
            if self._thread.is_alive():
                self.error = (self.error or "") + " 钩子线程未能退出"
        self._thread = None

    # ---------------------------------------------------------------- 内部

    def _run(self) -> None:
        try:
            self._thread_id = kernel32.GetCurrentThreadId()
            self._cb = HOOKPROC(self._hook_proc)
            # LL 钩子的 hMod 只要求"包含钩子过程的模块句柄"；回调由 ctypes
            # 生成在实践中需传一个真实模块句柄，kernel32 是最稳的选择。
            hmod = kernel32.GetModuleHandleW("kernel32.dll")
            hook = user32.SetWindowsHookExW(WH_KEYBOARD_LL, self._cb, hmod, 0)
            if not hook:
                self.error = f"SetWindowsHookExW err={ctypes.get_last_error()}"
                self._ready.set()
                return
            self._hook = hook
            self.installed = True
            self._ready.set()
            msg = MSG()
            while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
                user32.TranslateMessage(ctypes.byref(msg))
                user32.DispatchMessageW(ctypes.byref(msg))
        except Exception as exc:  # noqa: BLE001
            self.error = f"{type(exc).__name__}: {exc}"
            self._ready.set()
        finally:
            if self._hook:
                user32.UnhookWindowsHookEx(self._hook)
                self._hook = None
                self.installed = False

    def _hook_proc(self, n_code, w_param, l_param):  # noqa: ANN001
        if n_code == HC_ACTION:
            try:
                kb = ctypes.cast(l_param, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
                flags = int(kb.flags)
                record = {
                    "t": time.time() * 1000.0,
                    "vk": int(kb.vkCode),
                    "scan": int(kb.scanCode),
                    "up": bool(flags & LLKHF_UP),
                    "flags": flags,
                    "injected": bool(flags & LLKHF_INJECTED),
                    "lower_il": bool(flags & LLKHF_LOWER_IL_INJECTED),
                    "altdown": bool(flags & LLKHF_ALTDOWN),
                    "extended": bool(flags & LLKHF_EXTENDED),
                }
                with self._lock:
                    self.events.append(record)
                if self._on_event is not None:
                    self._on_event(record)
            except Exception as exc:  # noqa: BLE001
                self.error = f"hook_proc: {type(exc).__name__}: {exc}"
        return user32.CallNextHookEx(self._hook, n_code, w_param, l_param)


def format_line(record: dict, phase: str = "") -> str:
    """统一的日志行格式：`|` 之前是机器可解析的 key=value，之后是人读的说明。"""
    head = (
        f"[LL] t={record['t']:.0f} phase={phase or '-'} "
        f"vk=0x{record['vk']:02X} up={int(record['up'])} "
        f"flags=0x{record['flags']:04X} injected={int(record['injected'])} "
        f"lower_il={int(record['lower_il'])} altdown={int(record['altdown'])}"
    )
    return f"{head} | {vk_label(record['vk'])}{' 释放' if record['up'] else ' 按下'}"


def count_pairs(events: list[dict], vk: int) -> dict:
    """统计某个虚拟键的按下/释放/严格配对，并识别 autorepeat 与孤立事件。

    **为什么必须按边沿去重，而不是直接数 DOWN/UP**
    按住一个键不放时，Windows 会持续投递 autorepeat 的按下消息。如果直接
    比较 DOWN 数与 UP 数，一次"按住 0.5 秒"会得到 1 个 UP 却有多个 DOWN，
    被误判成粘键。这里按时间序做**边沿跳变**判定：连续 DOWN 中的后续只计数
    为 repeat，不产生新的"待配对"状态。

    **配对是硬判据**：报告层产键最大的风险是释放沿丢失造成本地粘键
    （粘住的 Alt 会污染用户后续每一次键盘输入）。只数"按下成功"是不够的。
    """
    seq = sorted([e for e in events if e["vk"] == vk], key=lambda e: e["t"])
    downs = ups = pairs = repeats = stray_up = 0
    pending = False
    for e in seq:
        if e["up"]:
            ups += 1
            if pending:
                pairs += 1
                pending = False
            else:
                stray_up += 1          # 没有按下沿的孤立释放
        else:
            downs += 1
            if pending:
                repeats += 1           # autorepeat，不是新的按下沿
            else:
                pending = True
    return {
        "downs": downs,
        "ups": ups,
        "pairs": pairs,
        "repeats": repeats,
        "stray_up": stray_up,
        "stuck": pending,              # 结束时仍按着 => 粘键
    }


def self_test(manual_seconds: float = 0.0) -> int:
    """观测器自检：必须同时证明"判得出合成"与"判得出真实"。

    只用一个人人都在冷淡性 `SendInput` 注入做阳性对照是不够的——那只证明了
    `injected=1` 这条路通。本自检第二部分请用户按一次物理键，**必须读到
    injected=0**；读不到就说明观测器没有分辨力，后面所有结论都不成立。
    """
    watcher = LLKeyWatcher(on_event=lambda r: print("  " + format_line(r)))
    if not watcher.start():
        print(f"监听启动失败: {watcher.error}")
        return 1
    print("LL 钩子已安装（免提权）")

    probe_vk = 0x7C  # F13：本机没有任何应用响应它，零可见副作用
    print(f"\n[对照 1/2] SendInput 注入 {vk_label(probe_vk)}——期望 injected=1")
    time.sleep(0.4)
    ok_send = send_key(probe_vk)
    time.sleep(1.0)
    hits = [e for e in watcher.events if e["vk"] == probe_vk]
    control_injected = len(hits) >= 1 and all(e["injected"] for e in hits)
    print(f"  注入调用成功={ok_send} 捕获 {len(hits)} 条 "
          f"全部 injected=1: {control_injected}")
    if hits and not control_injected:
        print("  ⚠ 合成输入没有读到 injected=1 —— 观测器不可用。")

    watcher.events.clear()
    physical_ok = True
    if manual_seconds > 0:
        print(f"\n[对照 2/2] {manual_seconds:.0f} 秒内请按下**物理键盘**的 F13"
              f"（没有 F13 就用 {vk_label(0x24)}）——期望 injected=0")
        time.sleep(manual_seconds)
        phys = [e for e in watcher.events if not e["injected"]]
        print(f"  捕获 injected=0 的真实按键 {len(phys)} 条")
        if phys:
            for e in phys[:6]:
                print("    " + format_line(e))
        physical_ok = len(phys) >= 1
        if not physical_ok:
            print("  ⚠ 没读到任何 injected=0 的按键 —— 观测器只证明了单向分辨力。")

    watcher.stop()
    print("\n=== LL 观测器自检 ===")
    print(f"  合成输入判据 (injected=1): {'通过' if control_injected else '失败'}")
    if manual_seconds > 0:
        print(f"  真实按键判据 (injected=0): {'通过' if physical_ok else '未验证'}")
    ok = control_injected and physical_ok
    print(f"  结论: {'可用' if ok else '不可用，禁止用于正式采集'}")
    return 0 if ok else 2


if __name__ == "__main__":
    import argparse

    cli = argparse.ArgumentParser(description="LL 钩子按键观测器自检")
    cli.add_argument("--manual", type=float, default=0.0,
                     help="物理键盘对照窗口秒数（建议 8 秒）")
    args = cli.parse_args()
    raise SystemExit(self_test(args.manual))
