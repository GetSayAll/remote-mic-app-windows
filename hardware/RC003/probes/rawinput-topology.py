"""复核 §7.6-1（第三版·终局）：设备枚举层面的确定性取证。

前两版证明：目标设备在 Raw Input 设备列表里是 KEYBOARD 类型，
RIDI_DEVICEINFO 也报 KEYBOARD，与该设备只有键盘页 TLC 的结论一致。

本版补最后一步：确认系统里**根本不存在**该设备的 HID 类型 Raw Input 条目。
方法（纯注册表读取，不提权、不编译）：

  1. HKLM\\SYSTEM\\CurrentControlSet\\Enum\\HID\\ 下找 VID_2717 / PID_32B8 的所有实例
     -> 该设备注册了几个 HID 实例、各自 UpperFilters / Service
  2. 每个实例的 Device Parameters 里有没有 UsagePage / Usage
  3. 交叉：Raw Input 列表里该设备只出现 1 次（前两版已证），
     若 Enum\\HID 下也只有 1 个实例 -> "只有一个 TLC" 坐实

判据：
  - 若 Enum\\HID 下只有 1 个实例、且其 Service=kbdhid（键盘类驱动）
    -> 该设备在 OS 层面只作为键盘设备存在，**不存在可被 HID 通道路由的实例**。
       故 "HID 通道收不到报文" 不是因为用法页过滤，而是因为**根本没有 HID 通道**。
  - 若存在多个实例或 Service=hidusb -> 需要重新评估。
"""
import winreg
import sys

out = []
def p(s=""):
    out.append(str(s))

VID_PID_TOKENS = ["2717", "32B8"]

def walk(root, path, depth=0, maxdepth=4):
    """列出 path 下的子键名。"""
    try:
        k = winreg.OpenKey(root, path, 0, winreg.KEY_READ)
    except OSError:
        return []
    subs = []
    i = 0
    while True:
        try:
            subs.append(winreg.EnumKey(k, i))
        except OSError:
            break
        i += 1
    winreg.CloseKey(k)
    return subs

def read_vals(root, path):
    d = {}
    try:
        k = winreg.OpenKey(root, path, 0, winreg.KEY_READ)
    except OSError:
        return d
    i = 0
    while True:
        try:
            name, val, typ = winreg.EnumValue(k, i)
            d[name] = val
        except OSError:
            break
        i += 1
    winreg.CloseKey(k)
    return d

BASE = r"SYSTEM\CurrentControlSet\Enum\HID"
p("=== 1. HKLM\\%s 下与 VID_2717 / PID_32B8 匹配的实例 ===" % BASE)

top = walk(winreg.HKEY_LOCAL_MACHINE, BASE)
p("HID 类下顶层子键数: %d" % len(top))
matches = [t for t in top if all(tok in t.upper() for tok in VID_PID_TOKENS)]
p("匹配 VID+PID 的顶层键: %r" % matches)
p()

for m in matches:
    sub = walk(winreg.HKEY_LOCAL_MACHINE, BASE + "\\" + m)
    p("顶层键 %s -> %d 个实例" % (m, len(sub)))
    for s in sub:
        full = BASE + "\\" + m + "\\" + s
        vals = read_vals(winreg.HKEY_LOCAL_MACHINE, full)
        p("  实例: %s" % s)
        for key in ("Service", "Class", "ClassGUID", "Driver", "Mfg", "DeviceDesc"):
            if key in vals:
                p("    %-11s = %s" % (key, vals[key]))
        dp = full + r"\Device Parameters"
        dpv = read_vals(winreg.HKEY_LOCAL_MACHINE, dp)
        if dpv:
            p("    Device Parameters:")
            for k2, v2 in dpv.items():
                if isinstance(v2, bytes):
                    v2 = v2.hex(" ").upper()
                p("      %-14s = %s" % (k2, v2))
        p()

p("=== 2. 全 HID 类下 Service=kbdhid 的实例（键盘类驱动，不分 VID）===")
for t in top:
    for s in walk(winreg.HKEY_LOCAL_MACHINE, BASE + "\\" + t):
        vals = read_vals(winreg.HKEY_LOCAL_MACHINE, BASE + "\\" + t + "\\" + s)
        if str(vals.get("Service", "")).lower() == "kbdhid":
            p("  %s\\%s" % (t, s))
p()

p("=== 3. 该设备在 Enum\\USB / Enum\\BTHENUM 下是否另有实例 ===")
for alt in (r"SYSTEM\CurrentControlSet\Enum\USB",
            r"SYSTEM\CurrentControlSet\Enum\BTHENUM",
            r"SYSTEM\CurrentControlSet\Enum\BTHLE"):
    sub = walk(winreg.HKEY_LOCAL_MACHINE, alt)
    hit = [s for s in sub if all(tok in s.upper() for tok in VID_PID_TOKENS)]
    p("  %-46s 子键 %d, 匹配 %r" % (alt, len(sub), hit))

open("rawinput-topology.out", "w", encoding="utf-8").write("\n".join(out))
print("WROTE rawinput-topology.out  (%d lines)" % len(out))
