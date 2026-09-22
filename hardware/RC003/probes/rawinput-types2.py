"""复核 §7.6-1（第二版）：修掉第一版的两处读数偏差。

第一版问题：
  1. 类型分布里出现 `3` —— 超出 RIM_TYPEMOUSE(0)/KEYBOARD(1)/HID(2) 的取值。
     嫌疑：`GetRawInputDeviceList` 填充调用写在同一行两次（第 61、63 行），
     第一次用了错误的 size，可能污染了数组。这里只调用一次。
  2. 目标设备 usage_page/usage 取不到 —— 第一版只在 `dtype == 2` 时读 hid 段；
     若设备被归为 KEYBOARD，hid 段按定义不填充（合理）。
     这里改为：不论 dtype，都 dump 整个 RID_DEVICE_INFO 的原始字节，
     并同时把 union 三个视图都打印出来，看是否有残留数据。

同时补一项第一版没做的关键取证：
  `GetRawInputDeviceInfoW(RIDI_DEVICEINFO)` 对 KEYBOARD 类型设备返回的
  dwType 与列表里的 dwType 是否一致（交叉验证，排除读数错位）。
"""
import ctypes
from ctypes import wintypes
import sys, collections

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
user32.GetRawInputDeviceList.restype = wintypes.UINT
user32.GetRawInputDeviceInfoW.restype = wintypes.UINT

TYPE_NAME = {0: "MOUSE", 1: "KEYBOARD", 2: "HID"}
RIDI_DEVICEINFO = 0x2000000B
RIDI_DEVICENAME = 0x20000007

out = []
def p(s=""):
    out.append(str(s))

# ---- 1. 列表（只填充一次，且严格用 sizeof(RAWINPUTDEVICELIST)）----
count = wintypes.UINT(0)
sz_list = ctypes.sizeof(RAWINPUTDEVICELIST)
r = user32.GetRawInputDeviceList(None, ctypes.byref(count), sz_list)
p("sizeof(RAWINPUTDEVICELIST) = %d" % sz_list)
p("GetRawInputDeviceList(size query) -> %s  count=%d" % (r, count.value))
if r == 0xFFFFFFFF or count.value == 0:
    p("FATAL: size query failed")
    open("rawinput-types2.out", "w", encoding="utf-8").write("\n".join(out))
    sys.exit(2)

arr = (RAWINPUTDEVICELIST * count.value)()
got = user32.GetRawInputDeviceList(arr, ctypes.byref(count), sz_list)
p("GetRawInputDeviceList(fill) -> %s (期望 == %d)" % (got, count.value))
p()

hits = []
for i in range(got):
    h = arr[i].hDevice
    list_dtype = arr[i].dwType

    # 设备名
    name_len = wintypes.UINT(0)
    user32.GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, None, ctypes.byref(name_len))
    name = ""
    if name_len.value:
        buf = ctypes.create_unicode_buffer(name_len.value + 1)
        user32.GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, buf, ctypes.byref(name_len))
        name = buf.value

    # 设备信息：不论 dtype 都取，并 dump 原始字节
    info = RID_DEVICE_INFO()
    ctypes.memset(ctypes.byref(info), 0xCC, ctypes.sizeof(RID_DEVICE_INFO))
    info.cbSize = ctypes.sizeof(RID_DEVICE_INFO)
    ilen = wintypes.UINT(ctypes.sizeof(RID_DEVICE_INFO))
    ok = user32.GetRawInputDeviceInfoW(h, RIDI_DEVICEINFO, ctypes.byref(info), ctypes.byref(ilen))

    raw = bytes(ctypes.string_at(ctypes.byref(info), ctypes.sizeof(RID_DEVICE_INFO)))
    hits.append(dict(
        idx=i, h=h, list_dtype=list_dtype,
        type=TYPE_NAME.get(list_dtype, "?%d" % list_dtype),
        name=name, ok=ok, ilen=ilen.value,
        info_dtype=info.dwType,
        usage_page=info.hid.usUsagePage, usage=info.hid.usUsage,
        vid=info.hid.dwVendorId, pid=info.hid.dwProductId,
        u_mouse=dict(dwId=info.mouse.dwId, nButtons=info.mouse.dwNumberOfButtons,
                     sampleRate=info.mouse.dwSampleRate),
        u_kb=dict(dwType=info.keyboard.dwType, dwSubType=info.keyboard.dwSubType,
                  dwKeyboardMode=info.keyboard.dwKeyboardMode,
                  dwNumberOfFunctionKeys=info.keyboard.dwNumberOfFunctionKeys,
                  dwNumberOfIndicators=info.keyboard.dwNumberOfIndicators,
                  dwNumberOfKeysTotal=info.keyboard.dwNumberOfKeysTotal),
        raw=raw,
    ))

p("总设备数: %d" % got)
p()
p("=== 全部 Raw Input 设备 ===")
p("%-4s %-9s %-8s %-6s %s" % ("idx", "type", "infoType", "ok", "name"))
for x in hits:
    it = TYPE_NAME.get(x["info_dtype"], "?%d" % x["info_dtype"])
    p("%-4d %-9s %-8s %-6s %s" % (x["idx"], x["type"], it, x["ok"], (x["name"] or "")[:120]))

p()
p("=== 目标设备（名字含 2717）===")
targets = [x for x in hits if "2717" in (x["name"] or "").upper()]
if not targets:
    p("未找到！")
for x in targets:
    p("  list_dtype  : %d (%s)" % (x["list_dtype"], x["type"]))
    p("  info.dwType : %d (%s)" % (x["info_dtype"], TYPE_NAME.get(x["info_dtype"], "?")))
    p("  ilen         : %d" % x["ilen"])
    p("  name         : %s" % x["name"])
    p("  [HID 视图] usage_page=0x%04X usage=0x%04X vid=0x%04X pid=0x%04X"
      % (x["usage_page"], x["usage"], x["vid"], x["pid"]))
    p("  [KB 视图]  subType=%d keysTotal=%d funcKeys=%d"
      % (x["u_kb"]["dwSubType"], x["u_kb"]["dwNumberOfKeysTotal"], x["u_kb"]["dwNumberOfFunctionKeys"]))
    p("  [原始字节] %s" % x["raw"].hex(" ").upper())
    p()

p("=== 类型分布（按列表 dwType）===")
c = collections.Counter(x["type"] for x in hits)
for k, v in sorted(c.items()):
    p("  %-10s %d" % (k, v))

open("rawinput-types2.out", "w", encoding="utf-8").write("\n".join(out))
print("WROTE rawinput-types2.out")
