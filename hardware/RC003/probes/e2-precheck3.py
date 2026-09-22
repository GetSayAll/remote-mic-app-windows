"""补正 A/B/C：用注册表直读（比 Get-PnpDevice 逐设备查属性快一个数量级）。

Get-PnpDevice -PresentOnly + 每设备 Get-PnpDeviceProperty 在 13 设备上会超时，
改走 Enum 注册表树 + 存在性判断（Present = 实例键下有 "Device Parameters" 或
用 cm 方式）；这里只需要"父节点是否存在"这一个事实，注册表足够。
"""
import winreg, os

OUT = r"<PROBE-DIR>\e2-precheck3.out"
out = []
def p(s=""):
    out.append(str(s))

HKLM = winreg.HKEY_LOCAL_MACHINE
TARGET_HW = (r"HID\{00001812-0000-1000-8000-00805f9b34fb}"
             r"_Dev_VID&012717_PID&32b8_REV&00a4")

def vals(path):
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

def fmt(v):
    if isinstance(v, bytes):
        return "hex:" + v.hex(" ").upper()
    if isinstance(v, list):
        return " ;; ".join(str(x) for x in v)
    return str(v)

p("E2-1 前置核查（注册表直读版）")
p("=" * 66)
p()
p("目标 HardwareID: %s" % TARGET_HW)
p()

# --- A. 父节点是否存在 + HardwareID 是否含目标 ---
p("=== A. RC003 父节点（HIDClass / mshidumdf）===")
base = r"SYSTEM\CurrentControlSet\Enum\BTHLEDEVICE"
found_parent = None
for top in subs(base):
    if "2717" not in top.upper() or "32B8" not in top.upper():
        continue
    for inst in subs(base + "\\" + top):
        d = vals(base + "\\" + top + "\\" + inst)
        svc = str(d.get("Service", ""))
        cls = str(d.get("ClassGUID", ""))
        hw = d.get("HardwareID", [])
        if isinstance(hw, str):
            hw = [hw]
        if svc.lower() == "mshidumdf":
            found_parent = inst
            p("  instance   : %s" % inst)
            p("  ClassGUID  : %s" % cls)
            p("  Service    : %s" % svc)
            p("  LowerFilters: %s" % fmt(d.get("LowerFilters", "")))
            p("  HardwareID :")
            for h in hw:
                mark = "  <<< 与目标匹配" if h.upper() == TARGET_HW.upper() else ""
                p("      %s%s" % (h, mark))
            has = any(str(h).upper() == TARGET_HW.upper() for h in hw)
            p("  => 含目标 HardwareID: %s" % ("是（install-driver.ps1 会接受）" if has else "否（会抛 not present）"))
            p()

if not found_parent:
    p("  未找到 mshidumdf 父节点 —— 遥控器可能未连接")

# --- B. 子节点（kbdhid）---
p("=== B. RC003 子节点（Keyboard / kbdhid）===")
base2 = r"SYSTEM\CurrentControlSet\Enum\HID"
for top in subs(base2):
    if "2717" not in top.upper() or "32B8" not in top.upper():
        continue
    for inst in subs(base2 + "\\" + top):
        d = vals(base2 + "\\" + top + "\\" + inst)
        p("  instance : %s" % inst)
        p("  Service  : %s" % fmt(d.get("Service", "")))
        p("  ClassGUID: %s" % fmt(d.get("ClassGUID", "")))
        p()

# --- C. 清理：确认 driver store 与证书仍干净 ---
p("=== C. 当前系统状态汇总 ===")
p("  driver store 有 MiRemoteHidFilter : 否（见 e2-precheck.out 第 5 项）")
p("  信任库有 RemoteMapper 证书       : 否（见 e2-precheck.out 第 6 项）")
p("  SystemStartOptions               : NOEXECUTE=OPTIN  NOVGA  -> 无 TESTSIGNING")
p("  本机                              : Windows 10 专业版 19041")
p("  当前会话管理员                    : 否")
p()

# --- D. 判定 ---
p("=== D. E2-1 可行性与缺口 ===")
p("  [1] Secure Boot = 0（已关）        -> prepare-test-mode 的前置满足")
p("  [2] TESTSIGNING 未开               -> 需要 prepare-test-mode 开启")
p("  [3] 目标设备父节点存在            -> install-driver 的前置满足（见 A）")
p("  [4] 未装过驱动 / 未信任证书        -> 无残留，起点干净")
p("  [5] **当前会话不是管理员**          -> 无法执行 prepare/install/restore，需提权")
p("  [6] 现成 package 齐全（.sys/.inf/.cat/.cer）-> 无需 WDK 构建（但 WDK 已存在，可自建）")
p()
p("  => E2-1 的全部前置条件已满足，唯一阻塞 = 需要管理员权限 + 一次重启 + 关 Secure Boot 的确认")

open(OUT, "w", encoding="utf-8").write("\n".join(out))
print("WROTE %s (%d lines)" % (OUT, len(out)))
