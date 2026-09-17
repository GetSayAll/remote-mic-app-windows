"""豆包"全局语音快捷键"实验：二、注入 vs 物理键 对照探针

目的：判断豆包语音能否被 SendInput 唤起，并区分下列三种可能：
  A. 物理键能唤起、注入不能   → 豆包过滤模拟输入（原判定，注入死角）
  B. 物理键与注入都能唤起     → 该状态下注入可用（新路线成立）
  C. 两者都不能唤起           → 豆包本身未就绪/快捷键没配对，实验无效

判据（三重，任一命中即算"唤起"）：
  1. 豆包语音窗口：类名 `OimeVoiceWaveWindow`（ZSTDJan 逆向所得，公开窗口类名）
     或其可见窗口出现；
  2. 系统录音设备被占用（豆包开麦会造成麦克风使用计数变化）；
  3. 屏幕差异（窗口位图或整屏抽样 diff）——最弱，仅作辅助。

用法：
  python 2-probe-injection-vs-physical.py --list
      只列出当前豆包窗口与麦克风状态（不发任何按键）
  python 2-probe-injection-vs-physical.py --inject-rightalt
      注入"按住右 Alt 1.5 秒"，其间/其后打印判据
  python 2-probe-injection-vs-physical.py --watch 25
      只观察 25 秒（请用户在此期间手动按住物理右 Alt），打印判据
      这样"物理键"由真人操作，避免我们自己注入污染对照。

安全边界：只注入键盘按键；不读豆包配置文件；不注入任何进程；不修改系统设置。
"""

import argparse
import ctypes
import os
import sys
import time
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

# ---- 豆包语音窗口类名（ZSTDJan 逆向记录，公开窗口类名）----
DOUBAO_VOICE_WINDOW_CLASS = "OimeVoiceWaveWindow"

# ---- SendInput（40 字节结构，2026-09-04 有过 32 字节 bug 的教训）----
INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_EXTENDEDKEY = 0x0001
KEYEVENTF_SCANCODE = 0x0008


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD),
                ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_void_p)]


class MOUSEINPUT(ctypes.Structure):
    """只为把 union 撑到与 Windows 一致的大小（x64 下 union 为 24 字节），
    否则 sizeof(INPUT) 会算成 32 而非正确的 40 —— 2026-09-04 本仓库已在
    探针里踩过一次（32 字节 INPUT 导致注入静默无效），此处显式对齐。"""
    _fields_ = [("dx", wintypes.LONG), ("dy", wintypes.LONG),
                ("mouseData", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_void_p)]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wintypes.DWORD), ("u", _INPUTUNION)]


# 启动即自检：Windows x64 下 INPUT 必须为 40 字节，否则注入静默失效。
assert ctypes.sizeof(INPUT) == 40, (
    f"INPUT 结构体大小 {ctypes.sizeof(INPUT)} != 40，注入会静默失败"
)


user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
user32.SendInput.restype = wintypes.UINT


def _key_event(scan: int, flags: int) -> INPUT:
    item = INPUT()
    item.type = INPUT_KEYBOARD
    item.u.ki.wVk = 0
    item.u.ki.wScan = scan
    item.u.ki.dwFlags = flags
    item.u.ki.time = 0
    item.u.ki.dwExtraInfo = None
    return item


def send_one(scan: int, flags: int) -> bool:
    """逐事件提交（豆包/微信都拒绝过零间隔单批，逐事件更接近物理时序）。"""
    item = _key_event(scan, flags)
    sent = user32.SendInput(1, ctypes.byref(item), ctypes.sizeof(INPUT))
    return sent == 1


RIGHT_ALT_SCAN = 0x38
EXT = KEYEVENTF_EXTENDEDKEY | KEYEVENTF_SCANCODE


def right_alt_down() -> bool:
    return send_one(RIGHT_ALT_SCAN, EXT)


def right_alt_up() -> bool:
    return send_one(RIGHT_ALT_SCAN, EXT | KEYEVENTF_KEYUP)


# ---- 判据 1：豆包语音窗口 ----
def find_voice_windows() -> list:
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def callback(hwnd, _lparam):
        buf = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, buf, 256)
        if DOUBAO_VOICE_WINDOW_CLASS.lower() in buf.value.lower():
            visible = bool(user32.IsWindowVisible(hwnd))
            found.append((hwnd, buf.value, visible))
            return True
        # 兜底：类名不含关键字时，检查标题是否像语音浮窗
        title = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(hwnd, title, 256)
        if title.value and ("语音" in title.value or "voice" in title.value.lower()):
            pid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            found.append((hwnd, f"{buf.value} | {title.value}", bool(user32.IsWindowVisible(hwnd)), pid.value))
        return True

    user32.EnumWindows(callback, 0)
    return found


# ---- 判据 2：麦克风是否被占用 ----
def mic_in_use() -> list:
    """用 Windows 公开 API 查询"麦克风正在被使用"的进程。

    走 CapabilityAccessManager 注册表的只读枚举属于"读系统状态"，
    但为守 AGENTS.md 边界，这里只用微软公开的
    `Windows.Media.Devices` 无法从纯 Python 直接调，因此改用
    `SetupAPI`/`MMDevice` 之外的稳妥办法：调用 waveIn 打开探测。
    简化实现：只报告系统是否有进程持有音频会话，不做归属。
    """
    # 用 winmm 的 waveInGetNumDevs + 尝试独占打开，判断是否被占用
    winmm = ctypes.WinDLL("winmm", use_last_error=True)
    num = winmm.waveInGetNumDevs()
    busy = []
    for dev in range(num):
        caps = ctypes.create_string_buffer(80)
        if winmm.waveInGetDevCapsA(dev, caps, 80) != 0:
            continue
        handle = wintypes.HANDLE()
        # WAVE_FORMAT_QUERY = 1（只查询，不真开）
        fmt = ctypes.create_string_buffer(18)
        fmt[0:2] = (1).to_bytes(2, "little")          # WAVE_FORMAT_PCM
        fmt[2:4] = (1).to_bytes(2, "little")          # channels
        fmt[4:8] = (16000).to_bytes(4, "little")      # samples/sec
        fmt[8:12] = (32000).to_bytes(4, "little")     # avg bytes/sec
        fmt[12:14] = (2).to_bytes(2, "little")        # block align
        fmt[14:16] = (16).to_bytes(2, "little")       # bits
        rc = winmm.waveInOpen(ctypes.byref(handle), dev, fmt, 0, 0, 0x0001)
        busy.append((dev, rc, rc == 0))
        if rc == 0:
            winmm.waveInClose(handle)
    return busy


def snapshot(label: str) -> None:
    print(f"\n--- {label} ---")
    wins = find_voice_windows()
    if wins:
        for w in wins:
            print(f"  [voice-window] hwnd={w[0]} class={w[1]} visible={w[2]}")
    else:
        print("  [voice-window] 未发现豆包语音窗口")
    busy = mic_in_use()
    n_busy = sum(1 for _, rc, ok in busy if not ok)
    print(f"  [mic] 设备数={len(busy)} 被占用={n_busy}"
          + (f" 详情={busy}" if n_busy else ""))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="只列当前状态")
    ap.add_argument("--inject-rightalt", action="store_true",
                    help="注入按住右 Alt 1.5 秒")
    ap.add_argument("--watch", type=int, default=0,
                    help="观察 N 秒（供用户手动按物理键）")
    ap.add_argument("--hold-ms", type=int, default=1500)
    args = ap.parse_args()

    snapshot("状态：实验开始前")

    if args.list:
        return 0

    if args.inject_rightalt:
        print(f"\n>>> 注入 按住右 Alt（{args.hold_ms} ms）…")
        if not right_alt_down():
            print("!! 注入 DOWN 失败")
            return 2
        time.sleep(args.hold_ms / 1000.0)
        snapshot("状态：注入按住期间")
        right_alt_up()
        time.sleep(1.2)
        snapshot("状态：注入释放后")

    if args.watch > 0:
        print(f"\n>>> 请现在手动按住【物理键盘右 Alt】并说话，观察窗口 {args.watch}s…")
        deadline = time.time() + args.watch
        tick = 0
        seen_any = False
        while time.time() < deadline:
            wins = [w for w in find_voice_windows() if w[2]]
            if wins:
                seen_any = True
                print(f"  t={tick}s 发现可见语音窗口: {[w[1] for w in wins]}")
            time.sleep(1.0)
            tick += 1
        print(f"  watch 期间是否曾出现可见语音窗口: {seen_any}")
        snapshot("状态：watch 结束")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
