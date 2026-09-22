"""查清 HardwareID 前缀不一致：目标串 HID\\{00001812-...} 到底匹配哪个节点。

已知：
  - 父节点（BTHLEDEVICE 树, mshidumdf）HardwareID 前缀 = BTHLEDevice\\
  - 子节点（HID 树, kbdhid）HardwareID 含 HID\\VID_2717&UP:0001_U:0006
  - RemoteMapper INF / install-driver.ps1 匹配 = HID\\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4

问题：这个目标串是否存在于任何节点的 HardwareID / CompatibleIDs 里？
"""
import winreg, os

OUT = r"<PROBE-DIR>\e2-hwid.out"
out = []
def p(s=""):
    out.append(str(s))

HKLM = winreg.HKEY_LOCAL_MACHINE
TARGET = (r"HID\{00001812-0000-1000-8000-00805f9b34fb}"
          r"_Dev_VID&012717_PID&32b8_REV&00a4")
TARGET_UP = TARGET.upper()

def vals(path):
    d = {}
    try:
        k = winreg.OpenKey(HKLM, path, 0, winreg.KEY_READ)
    except OSError:
        return {}
    i = 0
    while True:
        try:
            n, v, t = winreg.EnumValue(k, i)
            d[n] = v
        except OSError:
            break
        i += 1
    winreg.CloseKey(k)
    return d

def subs(path):
    try:
        k = winreg.OpenKey(HKLM, path, 0, winreg.KEY_READ)
    except OSError:
        return []
    r, i = [], 0
    while True:
        try:
            r.append(winreg.EnumKey(k, i))
        except OSError:
            break
        i += 1
    winreg.CloseKey(k)
    return r

def aslist(v):
    if v is None:
        return []
    return v if isinstance(v, list) else [v]

p("硬编码目标串: %s" % TARGET)
p("=" * 70)
p()

# 遍历两棵树，列出全部节点的 HardwareID / CompatibleIDs，标注是否命中
for tree, label in ((r"SYSTEM\CurrentControlSet\Enum\BTHLEDEVICE", "BTHLEDEVICE"),
                    (r"SYSTEM\CurrentControlSet\Enum\HID", "HID")):
    p("### 树 %s" % label)
    for top in subs(tree):
        if "2717" not in top.upper() or "32B8" not in top.upper():
            continue
        for inst in subs(tree + "\\" + top):
            d = vals(tree + "\\" + top + "\\" + inst)
            hw = aslist(d.get("HardwareID"))
            cid = aslist(d.get("CompatibleIDs"))
            svc = d.get("Service", "")
            p("  实例 %s   Service=%s" % (inst, svc))
            for name, lst in (("HardwareID", hw), ("CompatibleIDs", cid)):
                for x in lst:
                    hit = "  <<<< 命中目标串" if str(x).upper() == TARGET_UP else ""
                    p("      %-14s %s%s" % (name, x, hit))
            # 前缀分析
            for x in hw:
                sx = str(x)
                if sx.startswith("HID\\"):
                    p("      [前缀] HID\\ ...  (与 INF 匹配串同前缀)")
                elif sx.startswith("BTHLEDevice\\"):
                    p("      [前缀] BTHLEDevice\\ ...  (与 INF 匹配串不同前缀)")
            p()

p("=" * 70)
p("### 结论")
p()
p("RemoteMapper 的 INF 第 24 行与 install-driver.ps1 第 6 行使用的匹配串：")
p("    HID\\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4")
p("而本机该设备父节点实际 HardwareID 首项：")
p("    BTHLEDevice\\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4")
p()
p("两串前缀不同（HID\\ vs BTHLEDevice\\）。若本机所有节点的")
p("HardwareID/CompatibleIDs 都不含带 HID\\ 前缀的那一串，")
p("则 install-driver.ps1 会抛 not present —— 该 blocker 必须在下发前解决。")

open(OUT, "w", encoding="utf-8").write("\n".join(out))
print("WROTE %s (%d lines)" % (OUT, len(out)))
