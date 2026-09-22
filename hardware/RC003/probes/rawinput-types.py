"""复核 §7.6-1：该设备到底以哪种 Raw Input 类型出现？

问题：`RIDEV_INPUTSINK` 只收注册过的用法页。设备若以别的用法页发 HID 报告，
会被静默丢弃 —— 观测结果与"从不产生 HID"相同，但结论相反。

方法（只读，不改产品代码、不提权）：
  1. 枚举 Raw Input 设备列表（GetRawInputDeviceList）
  2. 对每个设备取 RIDI_DEVICEINFO 与 RIDI_DEVICENAME
  3. 找出 PID_32b8 那个，打印它的 dwType（RIM_TYPEMOUSE=0 / KEYBOARD=1 / HID=2）
  4. 再取 RIDI_DEVICEINFO 里的 HID 段：usUsagePage / usUsage

这一步直接回答"它是被归为键盘还是 HID"，不依赖按键实验。
"""
import ctypes
from ctypes import wintypes
import sys

# --- 结构体 ---
class RAWINPUTDEVICELIST(ctypes.Structure):
    _fields_ = [("hDevice", wintypes.HANDLE), ("dwType", wintypes.DWORD)]

class RID_DEVICE_INFO_MOUSE(ctypes.Structure):
    _fields_ = [("dwId", wintypes.DWORD), ("dwNumberOfButtons", wintypes.DWORD),
                ("dwSampleRate", wintypes.DWORD), ("fHasHorizontalWheel", wintypes.BOOL)]

class RID_DEVICE_INFO_KEYBOARD(ctypes.Structure):
    _fields_ = [("dwType", wintypes.DWORD), ("dwSubType", wintypes.DWORD),
                ("dwKeyboardMode", wintypes.DWORD), ("dwNumberOfFunctionKeys", wintypes.DWORD),
                ("dwNumberOfIndicators", wintypes.DWORD), ("dwNumberOfKeysTotal", wintypes.DWORD)]

class RID_DEVICE_INFO_HID(ctypes.Structure):
    _fields_ = [("dwVendorId", wintypes.DWORD), ("dwProductId", wintypes.DWORD),
                ("dwVersionNumber", wintypes.DWORD), ("usUsagePage", wintypes.USHORT),
                ("usUsage", wintypes.USHORT)]

class _RID_DEVICE_INFO_U(ctypes.Union):
    _fields_ = [("mouse", RID_DEVICE_INFO_MOUSE),
                ("keyboard", RID_DEVICE_INFO_KEYBOARD),
                ("hid", RID_DEVICE_INFO_HID)]

class RID_DEVICE_INFO(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("cbSize", wintypes.DWORD), ("dwType", wintypes.DWORD),
                ("u", _RID_DEVICE_INFO_U)]

user32 = ctypes.windll.user32
TYPE_NAME = {0: "MOUSE", 1: "KEYBOARD", 2: "HID"}
RIDI_DEVICEINFO = 0x2000000B
RIDI_DEVICENAME = 0x20000007

# 1. 列表
count = wintypes.UINT(0)
sz = ctypes.sizeof(RAWINPUTDEVICELIST)
r = user32.GetRawInputDeviceList(None, ctypes.byref(count), sz)
print("GetRawInputDeviceList(size query) ->", r, "count =", count.value)
if r == 0xFFFFFFFF:
    print("FATAL: size query failed")
    sys.exit(2)

arr = (RAWINPUTDEVICELIST * count.value)()
got = user32.GetRawInputDeviceList(arr, ctypes.byref(count), sz if False else sz)
# 修正：第二个 call 的最后一个参数是 sizeof(RAWINPUTDEVICELIST)
got = user32.GetRawInputDeviceList(arr, ctypes.byref(count), ctypes.sizeof(RAWINPUTDEVICELIST))
print("GetRawInputDeviceList(fill) ->", got)
print()

hits = []
for i in range(got):
    h = arr[i].hDevice
    dtype = arr[i].dwType

    # 设备名
    name_len = wintypes.UINT(0)
    user32.GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, None, ctypes.byref(name_len))
    name = ""
    if name_len.value:
        buf = ctypes.create_unicode_buffer(name_len.value + 1)
        user32.GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, buf, ctypes.byref(name_len))
        name = buf.value

    # 设备信息
    info = RID_DEVICE_INFO()
    info.cbSize = ctypes.sizeof(RID_DEVICE_INFO)
    ilen = wintypes.UINT(ctypes.sizeof(RID_DEVICE_INFO))
    ok = user32.GetRawInputDeviceInfoW(h, RIDI_DEVICEINFO, ctypes.byref(info), ctypes.byref(ilen))

    row = {
        "type": TYPE_NAME.get(dtype, str(dtype)),
        "name": name,
        "ok": ok,
        "usage_page": None,
        "usage": None,
        "vid": None,
        "pid": None,
    }
    if ok and dtype == 2:
        row["usage_page"] = "0x%02X" % info.hid.usUsagePage
        row["usage"] = "0x%02X" % info.hid.usUsage
        row["vid"] = "0x%04X" % info.hid.dwVendorId
        row["pid"] = "0x%04X" % info.hid.dwProductId
    hits.append(row)

print("总设备数:", got)
print()
print("=== 全部 Raw Input 设备 ===")
for r_ in hits:
    hn = "yes" if "2717" in (r_["name"] or "").upper() else "no"
    print("%-9s vid2717=%-3s %s" % (r_["type"], hn, (r_["name"] or "")[:130]))

print()
print("=== 目标设备（名字含 2717 / 32b8）===")
target = [r_ for r_ in hits if "2717" in (r_["name"] or "").upper()]
if not target:
    print("未找到。")
for r_ in target:
    print("  type       :", r_["type"])
    print("  name       :", r_["name"])
    print("  usage_page :", r_["usage_page"])
    print("  usage      :", r_["usage"])
    print("  vid/pid    :", r_["vid"], r_["pid"])
    print()

# 汇总各类型
import collections
c = collections.Counter(r_["type"] for r_ in hits)
print("=== 类型分布 ===")
for k, v in c.items():
    print("  %-10s %d" % (k, v))
