"""E2-1 前置核查：本机是否已具备挂 lower filter 的条件，以及当前系统状态。

全部只读，不改任何东西。

检查项：
 1. Secure Boot 状态（prepare-test-mode 会因它开启而拒绝）
 2. TESTSIGNING 当前是否已开（bcdedit /enum {current}）
 3. 目标设备（RC003 父节点）当前是否 present（install-driver 的前置）
 4. 是否已装过 MiRemoteHidFilter（避免重复装）
 5. LocalMachine Root / TrustedPublisher 里是否已有该测试证书
 6. 管理员权限（当前会话是否提权）
 7. WDK / VS Build Tools 是否存在（如需自行构建；若用现成 package 则不需要）
 8. package/ 里的文件完整性（.sys/.inf/.cat/.cer）

注意：PowerShell 工具 stdout 可能不可见，本脚本用 Python 调 powershell 并捕获输出写文件。
"""
import subprocess, os, json, ctypes, datetime

OUT = r"<PROBE-DIR>\e2-precheck.out"
out = []
def p(s=""):
    out.append(str(s))

def ps(cmd, timeout=60):
    """中文 Windows 的 powershell 输出是 GBK/OEM 编码，必须容错解码。"""
    try:
        r = subprocess.run(["powershell", "-NoProfile", "-Command", cmd],
                           capture_output=True, timeout=timeout)
    except Exception as e:
        return "", "EXC:" + str(e), -1
    def dec(b):
        if not b:
            return ""
        for enc in ("utf-8", "gbk", "cp936", "latin-1"):
            try:
                return b.decode(enc).strip()
            except UnicodeDecodeError:
                continue
        return b.decode("utf-8", errors="replace").strip()
    return dec(r.stdout), dec(r.stderr), r.returncode

p("E2-1 前置核查  (%s)" % datetime.datetime.now().isoformat(timespec="seconds"))
p("=" * 62)
p()

# 1. 管理员权限
try:
    is_admin = bool(ctypes.windll.shell32.IsUserAnAdmin())
except Exception as e:
    is_admin = "ERR:" + str(e)
p("1. 当前会话是否管理员        : %s" % is_admin)
p()

# 2. Secure Boot
o, e, rc = ps("(Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State' "
              "-Name UEFISecureBootEnabled -ErrorAction SilentlyContinue).UEFISecureBootEnabled")
p("2. Secure Boot (UEFISecureBootEnabled): %r" % o)
p("   -> 1 = 开着（prepare-test-mode 会拒绝）；0/空 = 关着或无该键")
p()

# 3. TESTSIGNING
o, e, rc = ps("bcdedit /enum '{current}' | Select-String -Pattern 'testsigning|TestSigning'")
p("3. bcdedit testsigning       : %r" % o)
if e:
    p("   stderr: %s" % e[:200])
p()

# 4. 目标设备 presence
cmd = (
 "$t='HID\\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4';"
 "Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | ForEach-Object {"
 "  $ids=(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds "
 "-ErrorAction SilentlyContinue).Data;"
 "  if($ids -contains $t){ \"$($_.Status)|$($_.Class)|$($_.FriendlyName)|$($_.InstanceId)\" } }"
)
o, e, rc = ps(cmd)
p("4. 目标设备（RC003 父节点）present:")
if o:
    for line in o.splitlines():
        p("     %s" % line)
else:
    p("     （未找到 —— 遥控器可能未连接/休眠）")
if e:
    p("   stderr: %s" % e[:200])
p()

# 4b. 顺便看 kbdhid 子节点
cmd2 = (
 "Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object "
 "{ $_.InstanceId -like '*VID&012717*32b8*' } | ForEach-Object "
 "{ \"$($_.Status)|$($_.Class)|$($_.FriendlyName)\" }"
)
o, e, rc = ps(cmd2)
p("4b. 所有 2717/32b8 相关 present 设备:")
if o:
    for line in o.splitlines():
        p("     %s" % line)
else:
    p("     （无）")
p()

# 5. 是否已装 MiRemoteHidFilter
o, e, rc = ps("Get-WindowsDriver -Online -All -ErrorAction SilentlyContinue | "
              "Where-Object { $_.OriginalFileName -like '*MiRemoteHidFilter*' } | "
              "Select-Object -ExpandProperty OriginalFileName")
p("5. driver store 里的 MiRemoteHidFilter : %r" % o)
p()

# 6. 证书是否已在信任库
o, e, rc = ps("foreach($s in @('Root','TrustedPublisher')){ "
              "Get-ChildItem \"Cert:\\LocalMachine\\$s\" -ErrorAction SilentlyContinue | "
              "Where-Object { $_.Subject -like '*RemoteMapper*' -or $_.Subject -like '*MiRemote*' } | "
              "ForEach-Object { \"$s|$($_.Subject)|$($_.Thumbprint)\" } }")
p("6. 信任库中的 RemoteMapper 测试证书:")
if o:
    for line in o.splitlines():
        p("     %s" % line)
else:
    p("     （无）")
p()

# 7. 构建工具链
for name, cmd in [
    ("WDK dir (Program Files (x86)\\Windows Kits\\10\\Include)",
     "Test-Path 'C:\\Program Files (x86)\\Windows Kits\\10\\Include\\10.0.26100.0'"),
    ("MSBuild",
     "(Get-Command msbuild -ErrorAction SilentlyContinue).Source"),
    ("VS BuildTools",
     "Test-Path 'C:\\Program Files (x86)\\Microsoft Visual Studio\\2022'"),
]:
    o, e, rc = ps(cmd)
    p("7. %-46s: %r" % (name, o))
p()

# 8. package 文件完整性
pkg = r"<USER-HOME>\ref-repos\RemoteMapper"
p("8. 现成 package 文件:")
names = ["driver__MiRemoteHidFilter__package__MiRemoteHidFilter.sys",
         "driver__MiRemoteHidFilter__package__MiRemoteHidFilter.inf",
         "driver__MiRemoteHidFilter__package__miremotehidfilter.cat",
         "driver__MiRemoteHidFilter__package__MiRemoteHidFilter.cer"]
for n in names:
    fp = os.path.join(pkg, n)
    if os.path.exists(fp):
        p("     %8d B  %s" % (os.path.getsize(fp), n.split("__")[-1]))
    else:
        p("     MISSING  %s" % n)
p()

open(OUT, "w", encoding="utf-8").write("\n".join(out))
print("WROTE %s (%d lines)" % (OUT, len(out)))
