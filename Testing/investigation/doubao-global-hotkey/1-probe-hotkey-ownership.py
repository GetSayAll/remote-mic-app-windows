"""豆包"免按模式"实验：一、系统热键注册探测

目的：在不读取/不修改豆包私有配置的前提下，判断豆包当前是否已向系统
      注册了语音热键。做法是"抢占式探测"——尝试自己 RegisterHotKey
      候选组合键；注册失败且 LastError=1409
      （ERROR_HOTKEY_ALREADY_REGISTERED）说明该组合已被别人注册。

何时用：用户切换豆包"语音输入模式"（长按模式 ↔ 免按模式）的前后各跑一次做对比。
        未切到免按模式时应全部 FREE；切换后若变 TAKEN，即证明豆包改用了
        RegisterHotKey 路径 → 意味着 SendInput 注入应当能触发。

背景（2026-09-17 定案）：配置项 voice.enableGlobalVoiceShortcut 在界面上
        对应「免按模式」（豆包设置 → 语音输入 → 语音输入模式），
        不是独立的"全局语音快捷键"开关。⚠️ 早期从 DLL 字符串/截图 OCR
        推断为「免提模式」，UIA 直读活窗口后确认为「免按模式」。
        **两档快捷键不同**：长按模式 = 纯右 Alt；免按模式 = 右 Alt + 空格。
        详见 Bugs/2026-09-17-doubao-global-shortcut-is-handsfree-mode.md。

本脚本只做 Register/Unregister，不注入按键、不改任何配置、不读豆包文件。
用 ctypes 而非 PowerShell Add-Type（本机 Add-Type 被安全策略硬拦）。
"""

import ctypes
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.RegisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int,
                                  wintypes.UINT, wintypes.UINT]
user32.RegisterHotKey.restype = wintypes.BOOL
user32.UnregisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int]
user32.UnregisterHotKey.restype = wintypes.BOOL

MOD_ALT, MOD_CONTROL, MOD_WIN = 0x1, 0x2, 0x8
VK_LMENU, VK_RMENU, VK_RSHIFT, VK_SPACE = 0xA4, 0xA5, 0xA1, 0x20
ERROR_HOTKEY_ALREADY_REGISTERED = 1409

CANDIDATES = [
    ("RightAlt (豆包长按语义)", MOD_ALT, VK_RMENU),
    # ⚠️ 同一 (modifiers, vk) 只能注册一次：重复项会在第二次注册时假报 1409
    #    → 假 TAKEN。所以「免按模式正解」(Alt+Space) 只保留这一条。
    ("RightAlt+Space (免按模式正解)", MOD_ALT, VK_SPACE),
    ("RightAlt+Win+Space (对照)", MOD_ALT | MOD_WIN, VK_SPACE),
    ("LeftAlt (对照)", MOD_ALT, VK_LMENU),
    ("RightShift (对照)", 0, VK_RSHIFT),
    ("Ctrl+Alt+R (对照)", MOD_CONTROL | MOD_ALT, 0x52),
]


def main() -> int:
    print("=== 系统热键占用探测 ===")
    taken = 0
    for index, (name, mods, vk) in enumerate(CANDIDATES, start=1):
        hotkey_id = 9100 + index
        if user32.RegisterHotKey(None, hotkey_id, mods, vk):
            user32.UnregisterHotKey(None, hotkey_id)
            print(f"  FREE      {name}")
        else:
            err = ctypes.get_last_error()
            taken += 1
            tag = ("TAKEN" if err == ERROR_HOTKEY_ALREADY_REGISTERED
                   else f"BLOCKED({err})")
            print(f"  {tag:<9} {name}  (LastError={err})")
    print(f"TAKEN 总数 = {taken}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
