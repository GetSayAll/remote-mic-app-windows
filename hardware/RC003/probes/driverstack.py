"""取证：RC003 的完整 PnP 设备树 + 各层过滤驱动槽位。

验证子代理的关键论断（不能只信转述）：
  1. RC003 是两级 PnP 节点：BTHLEDEVICE(HIDClass, mshidumdf) -> HID(Keyboard, kbdhid)
  2. 传输驱动是 UMDF 的 Microsoft.Bluetooth.Profiles.HidOverGatt.dll，不是 hidbth.sys
  3. HIDClass 类键 {745a17a0} 的 UpperFilters/LowerFilters 当前为空（可挂）
  4. Keyboard 类键 {4d36e96b} 的 UpperFilters=kbdclass
  5. kbdclass 的 ConnectMultiplePorts 参数（决定每个键盘是否独立栈）

全部只读注册表 + PnP 查询，不改任何东西。
"""
import winreg, os, json, subprocess

OUT = r"<PROBE-DIR>\driverstack.out"
out = []
def p(s=""):
    out.append(str(s))

HKLM = winreg.HKEY_LOCAL_MACHINE

def enum_vals(path):
    d = {}
    try:
        k = winreg.OpenKey(HKLM, path, 0, winreg.KEY_READ)
    except OSError as e:
        return {"__err__": str(e)}
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

def enum_subkeys(path):
    try:
        k = winreg.OpenKey(HKLM, path, 0, winreg.KEY_READ)
    except OSError as e:
        return ["__err__" + str(e)]
    r = []
    i = 0
    while True:
        try:
            r.append(winreg.EnumKey(k, i))
        except OSError:
            break
        i += 1
    winreg.CloseKey(k)
    return r

def fmt(v):
    if isinstance(v, bytes):
        return "hex:" + v.hex(" ").upper()
    if isinstance(v, list):
        return " | ".join(str(x) for x in v)
    return str(v)

def dump_key(path, title=None):
    p("--- %s" % (title or path))
    p("    KEY: HKLM\\" + path)
    d = enum_vals(path)
    if "__err__" in d:
        p("    [不可读] %s" % d["__err__"])
    else:
        for k in sorted(d):
            p("    %-24s = %s" % (k, fmt(d[k])))
    p()

# ================= 1. 枚举 RC003 的两棵设备树 =================
p("=" * 70)
p("1. RC003 设备树定位")
p("=" * 70)
p()

TOK = ["2717", "32B8"]

p("### 1.1 Enum\\HID 下匹配的键（子节点：kbdhid）")
hid_base = r"SYSTEM\CurrentControlSet\Enum\HID"
hid_top = [t for t in enum_subkeys(hid_base) if all(x in t.upper() for x in TOK)]
p("匹配: %r" % hid_top)
for t in hid_top:
    subs = enum_subkeys(hid_base + "\\" + t)
    for s in subs:
        dump_key(hid_base + "\\" + t + "\\" + s, "HID 子节点实例")
p()

p("### 1.2 Enum\\BTHLEDEVICE 下匹配的键（父节点）")
for base in (r"SYSTEM\CurrentControlSet\Enum\BTHLEDEVICE",
             r"SYSTEM\CurrentControlSet\Enum\BTHLE",
             r"SYSTEM\CurrentControlSet\Enum\BTHENUM"):
    subs = enum_subkeys(base)
    hit = [s for s in subs if all(x in s.upper() for x in TOK)]
    p("%s -> 子键 %d, 匹配 %r" % (base.split("\\")[-1], len(subs), hit))
    for h in hit:
        for inst in enum_subkeys(base + "\\" + h):
            dump_key(base + "\\" + h + "\\" + inst, "%s 实例" % base.split("\\")[-1])
p()

# ================= 2. 类键过滤驱动槽位 =================
p("=" * 70)
p("2. 类键过滤驱动槽位（挂载可行性）")
p("=" * 70)
p()
dump_key(r"SYSTEM\CurrentControlSet\Control\Class\{4d36e96b-e325-11ce-bfc1-08002be10318}",
         "Keyboard 类键 {4d36e96b}")
dump_key(r"SYSTEM\CurrentControlSet\Control\Class\{745a17a0-74d3-11d0-b6fe-00a0c90f57da}",
         "HIDClass 类键 {745a17a0}")
dump_key(r"SYSTEM\CurrentControlSet\Control\Class\{4d36e96b-e325-11ce-bfc1-08002be10318}\Properties",
         "Keyboard 类键 Properties（含 ConnectMultiplePorts）")
dump_key(r"SYSTEM\CurrentControlSet\Control\Keyboard Layout",
         "Keyboard Layout 全局")

p("### 2.1 全系统搜索 ConnectMultiplePorts / KeyboardDataQueueSize")
def search_reg(base, needle, paths=None, depth=0, maxd=3):
    found = []
    for sk in enum_subkeys(base):
        if sk.startswith("__err__"):
            continue
        full = base + "\\" + sk
        d = enum_vals(full)
        for k, v in d.items():
            if needle.lower() in k.lower():
                found.append((full, k, fmt(v)))
        if depth < maxd:
            found += search_reg(full, needle, None, depth + 1, maxd)
    return found

for nd in ("ConnectMultiplePorts", "KeyboardDataQueueSize"):
    hits = search_reg(r"SYSTEM\CurrentControlSet\Control\Class\{4d36e96b-e325-11ce-bfc1-08002be10318}", nd)
    p("  [%s] 命中 %d 处" % (nd, len(hits)))
    for f, k, v in hits[:10]:
        p("     HKLM\\%s  %s = %s" % (f, k, v))
p()

# ================= 3. 服务键 =================
p("=" * 70)
p("3. 相关服务键")
p("=" * 70)
p()
for svc in ("kbdhid", "kbdclass", "hidclass", "hidbth", "hidusb", "mshidumdf", "WUDFRd"):
    dump_key(r"SYSTEM\CurrentControlSet\Services\%s" % svc, "服务 %s" % svc)

open(OUT, "w", encoding="utf-8").write("\n".join(out))
print("WROTE %s (%d lines)" % (OUT, len(out)))
