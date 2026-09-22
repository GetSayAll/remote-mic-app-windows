"""补正 e2-precheck 的两项：目标设备 HardwareID 匹配、testsigning 真值。

第一版问题：
  4)  PowerShell 命令里的单引号被 bash/shell 处理吃掉 -> 改用文件传参
  9)  bcdedit 无输出 -> 可能需管理员，改用多种途径交叉
"""
import subprocess, os

OUT = r"<PROBE-DIR>\e2-precheck2.out"
out = []
def p(s=""):
    out.append(str(s))

def dec(b):
    if not b:
        return ""
    for enc in ("utf-8", "gbk", "cp936", "latin-1"):
        try:
            return b.decode(enc).strip()
        except UnicodeDecodeError:
            continue
    return b.decode("utf-8", errors="replace").strip()

def ps_script(script, timeout=90):
    """把脚本写文件再跑，彻底绕开 shell 引号问题。"""
    sf = r"<PROBE-DIR>\_tmp_ps.ps1"
    with open(sf, "w", encoding="utf-8-sig") as f:   # BOM：PS5.1 读 UTF-8 需要
        f.write(script)
    try:
        r = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                            "-File", sf],
                           capture_output=True, timeout=timeout)
    except Exception as e:
        return "", "EXC:" + str(e), -1
    return dec(r.stdout), dec(r.stderr), r.returncode

TARGET = r"HID\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4"

p("E2-1 前置核查（补正版）")
p("=" * 62)
p()
p("目标 HardwareID: %s" % TARGET)
p()

# --- A. 目标设备匹配（与 install-driver.ps1 第 15-18 行同逻辑）---
script = '''
$t = 'HID\\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4'
Write-Output "=== A. 与 install-driver.ps1 相同逻辑的匹配 ==="
$found = $false
Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | ForEach-Object {
  $ids = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds -ErrorAction SilentlyContinue).Data
  if ($ids -contains $t) {
    $script:found = $true
    Write-Output ("MATCH  status={0} class={1} name={2}" -f $_.Status, $_.Class, $_.FriendlyName)
    Write-Output ("       instance={0}" -f $_.InstanceId)
    Write-Output ("       allHWIDs={0}" -f ($ids -join ' ;; '))
  }
}
if (-not $found) { Write-Output "NO MATCH -- install-driver.ps1 会抛 'Target keyboard collection is not present'" }

Write-Output ""
Write-Output "=== B. RC003 相关全部 present 设备的 InstanceId + Class ==="
Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object {
  $_.InstanceId -like '*2717*' -or $_.InstanceId -like '*32b8*' -or $_.InstanceId -like '*32B8*'
} | ForEach-Object {
  Write-Output ("{0,-9} {1,-10} {2}" -f $_.Status, $_.Class, $_.InstanceId)
}

Write-Output ""
Write-Output "=== C. 父节点(mshidumdf) 与 子节点(kbdhid) 的 Service ==="
Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object {
  $_.InstanceId -like '*HID*2717*32b8*'
} | ForEach-Object {
  $svc = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_Service -ErrorAction SilentlyContinue).Data
  $cls = $_.Class
  Write-Output ("{0}`n    Service={1}  Class={2}" -f $_.InstanceId, $svc, $cls)
}
'''
o, e, rc = ps_script(script)
p(o if o else "（A/B/C 无输出）")
if e:
    p("stderr: %s" % e[:400])
p()

# --- D. testsigning 真值（多途径）---
script2 = '''
Write-Output "=== D. TESTSIGNING 真值 ==="
Write-Output "-- bcdedit /enum {current} 全文 --"
$b = & bcdedit.exe /enum '{current}' 2>&1 | Out-String
Write-Output $b
Write-Output ("-- exitcode={0} --" -f $LASTEXITCODE)
Write-Output ""
Write-Output "-- 注册表 SystemStartOptions --"
$sso = (Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control' -Name SystemStartOptions -ErrorAction SilentlyContinue).SystemStartOptions
Write-Output ("SystemStartOptions = {0}" -f $sso)
Write-Output ""
Write-Output "-- 当前 boot 是否测试模式：Win32_OperatingSystem --"
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
Write-Output ("BuildNumber={0}  Caption={1}" -f $os.BuildNumber, $os.Caption)
'''
o2, e2, rc2 = ps_script(script2)
p(o2 if o2 else "（D 无输出）")
if e2:
    p("stderr: %s" % e2[:400])

open(OUT, "w", encoding="utf-8").write("\n".join(out))
print("WROTE %s (%d lines)" % (OUT, len(out)))
