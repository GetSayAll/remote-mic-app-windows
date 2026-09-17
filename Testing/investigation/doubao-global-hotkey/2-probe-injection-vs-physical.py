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
WAVE_FORMAT_QUERY = 0x0001


def _wave_format() -> ctypes.Array:
    """构造一个 WAVEFORMATEX 缓冲区（18 字节，纯 PCM 16k/mono/16bit）。

    注意：`create_string_buffer(18)` 会实际分配 19 字节（尾部补 NUL），
    但长度字段传 18 即可——这里显式用 `ctypes.create_string_buffer(18)` 并
    按偏移写入，Windows 只读前 18 字节。
    """
    fmt = ctypes.create_string_buffer(18)
    fmt[0:2] = (1).to_bytes(2, "little")          # wFormatTag = WAVE_FORMAT_PCM
    fmt[2:4] = (1).to_bytes(2, "little")          # nChannels
    fmt[4:8] = (16000).to_bytes(4, "little")      # nSamplesPerSec
    fmt[8:12] = (32000).to_bytes(4, "little")     # nAvgBytesPerSec
    fmt[12:14] = (2).to_bytes(2, "little")        # nBlockAlign
    fmt[14:16] = (16).to_bytes(2, "little")       # wBitsPerSample
    fmt[16:18] = (0).to_bytes(2, "little")        # cbSize
    return fmt


def mic_in_use() -> list:
    """用公开的 winmm API 查询每个录音设备现在能否被打开。

    ⚠️ 必须用 WAVE_FORMAT_QUERY（0x0001）——它只做"这个格式支持吗"的**查询**，
    不真正占用设备，因此不会干扰被测应用开麦。传 0 则是真打开，
    本探针自己就会把设备占住，自证污染。

    返回 [(设备号, mmr 返回码)]；mmr == MMSYSERR_NOERROR(0) 表示该设备
    **当前可用**（未被独占），非 0 常见为 MMSYSERR_ALLOCATED(4)=已被占用。
    为了不误报，这里只报告每个设备的返回码，由调用方判断。
    """
    winmm = ctypes.WinDLL("winmm", use_last_error=True)
    winmm.waveInGetNumDevs.restype = wintypes.UINT
    winmm.waveInOpen.argtypes = [ctypes.POINTER(wintypes.HANDLE), wintypes.UINT,
                                 ctypes.c_void_p, ctypes.c_void_p,
                                 ctypes.c_void_p, wintypes.DWORD]
    winmm.waveInOpen.restype = wintypes.UINT

    num = winmm.waveInGetNumDevs()
    results = []
    for dev in range(num):
        handle = wintypes.HANDLE()
        fmt = _wave_format()
        rc = winmm.waveInOpen(ctypes.byref(handle), dev, ctypes.byref(fmt),
                              None, None, WAVE_FORMAT_QUERY)
        results.append((dev, rc))
    return results


def snapshot(label: str) -> None:
    print(f"\n--- {label} ---")
    wins = find_voice_windows()
    if wins:
        for w in wins:
            # (hwnd, class 或 "class | title", visible[, pid])
            cls = w[1]
            pid = w[3] if len(w) > 3 else None
            suffix = f" pid={pid}" if pid is not None else ""
            print(f"  [voice-window] hwnd={w[0]} class={cls} visible={w[2]}{suffix}")
    else:
        print("  [voice-window] 未发现豆包语音窗口")
    busy = mic_in_use()
    unavailable = [(d, rc) for d, rc in busy if rc != 0]
    print(f"  [mic] 录音设备数={len(busy)} 当前不可用={len(unavailable)}"
          + (f" 详情={unavailable}" if unavailable else ""))


def doubao_voice_window_visible() -> bool:
    """判据 1 的直接取值：只关心 `OimeVoiceWaveWindow` 这一条（判据强度最高）。

    与 `find_voice_windows` 的兜底分支不同，这里**只认类名精确匹配**，
    避免把微信输入法窗口（`wetype.flutter.setting`，标题含"语音输入"）
    误算成豆包语音窗口——那会让实验假阳性。
    """
    for w in find_voice_windows():
        if w[1].lower() == DOUBAO_VOICE_WINDOW_CLASS.lower():
            return bool(w[2])
    return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="只列当前状态")
    ap.add_argument("--inject-rightalt", action="store_true",
                    help="注入一次「按下+松开」完整点击（免按模式用；长按模式用 --hold-ms）")
    ap.add_argument("--hold-ms", type=int, default=0,
                    help="改为「按住 N 毫秒再松开」（长按模式语义）；0=只点一下")
    ap.add_argument("--toggle", type=int, default=0, metavar="N",
                    help="连续注入 N 次完整点击（免按模式必须成对：1 次开始、2 次结束）")
    ap.add_argument("--watch", type=int, default=0,
                    help="观察 N 秒（供用户手动按物理键）")
    ap.add_argument("--poll-ms", type=int, default=100,
                    help="watch 期间的判据轮询间隔（默认 100ms，"
                         "单次按键的语音窗口可能只闪几百毫秒，1s 会漏掉）")
    args = ap.parse_args()

    snapshot("状态：实验开始前")

    if args.list:
        return 0

    if args.toggle > 0:
        # 免按模式是切换式：必须在「开麦期间」采样，否则会漏掉判据。
        print(f"\n>>> 免按模式：连续注入 {args.toggle} 次完整点击")
        for index in range(1, args.toggle + 1):
            print(f"\n>>> 第 {index} 次点击（按下+松开）…")
            if not right_alt_down():
                print("!! 注入 DOWN 失败")
                return 2
            # 免按模式下"按下"即切换；稍等让豆包响应，再抬起。
            time.sleep(0.12)
            if not right_alt_up():
                print("!! 注入 UP 失败")
                return 2
            # 抬起后立刻采样：若第 1 次已开麦，此处应看到 visible=True。
            time.sleep(0.35)
            snapshot(f"状态：第 {index} 次点击后")

    if args.inject_rightalt:
        hold_ms = args.hold_ms or 1500
        print(f"\n>>> 注入 按住右 Alt（{hold_ms} ms）…")
        if not right_alt_down():
            print("!! 注入 DOWN 失败")
            return 2
        # 按住期间密集采样：语音窗口可能只闪一小段。
        deadline = time.time() + hold_ms / 1000.0
        seen = False
        while time.time() < deadline:
            if doubao_voice_window_visible():
                seen = True
            time.sleep(min(0.05, max(0.0, deadline - time.time())))
        snapshot("状态：注入按住期间")
        right_alt_up()
        time.sleep(1.2)
        snapshot("状态：注入释放后")
        print(f"  按住期间豆包语音窗口是否可见: {seen}")

    if args.watch > 0:
        print(f"\n>>> 请现在手动按住【物理键盘右 Alt】并说话，观察窗口 {args.watch}s…")
        deadline = time.time() + args.watch
        seen_any = False
        polls = 0
        while time.time() < deadline:
            polls += 1
            if doubao_voice_window_visible():
                if not seen_any:
                    print(f"  t≈{args.watch - (deadline - time.time()):.1f}s "
                          f"首次出现可见的可豆包语音窗口")
                seen_any = True
            time.sleep(args.poll_ms / 1000.0)
        print(f"  watch 期间轮询 {polls} 次；是否曾出现可见语音窗口: {seen_any}")
        snapshot("状态：watch 结束")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
