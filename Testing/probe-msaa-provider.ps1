# 无障碍接口存在性探针（一次性诊断工具，不进入产品、不随安装包发布）。
#
# 用途：判断某个窗口有没有向系统暴露无障碍接口。对窗口发送
# `WM_GETOBJECT`，分别请求 `OBJID_CLIENT`（旧版 MSAA/IAccessible）与
# `UiaRootObjectId`（UI Automation）。返回的 LRESULT 非 0 = 存在对应接口；
# 两者都为 0 = 该窗口完全不暴露无障碍对象（UIA 学不到任何输入框）。
#
# 实测（2026-10-15）：微信 4.x 主窗口与其渲染子窗口两者都是 0；Chrome 是
# 49543 / 65536（对照组）。这是「微信无法学习输入框」的根因证据。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-msaa-provider.ps1 -WindowHandle 0x3B0770

param(
    [Parameter(Mandatory = $true)][string]$WindowHandle
)

$ErrorActionPreference = 'Stop'

Add-Type -Namespace SayAllMsaa -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Auto)]
public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint msg, System.IntPtr wParam,
    System.IntPtr lParam, uint flags, uint timeout, out System.IntPtr result);
'@

$raw = $WindowHandle.Trim()
if ($raw.StartsWith('0x') -or $raw.StartsWith('0X')) {
    $hwnd = [IntPtr][Convert]::ToInt64($raw.Substring(2), 16)
} else {
    $hwnd = [IntPtr][Convert]::ToInt64($raw)
}

foreach ($pair in @(@('OBJID_CLIENT(-4)', -4), @('UiaRootObjectId(-25)', -25))) {
    $name = $pair[0]
    $objectId = [int]$pair[1]
    $result = [IntPtr]::Zero
    # WM_GETOBJECT = 0x003D；SMTO_ABORTIFHUNG = 0x0002。
    $ok = [SayAllMsaa.Native]::SendMessageTimeout($hwnd, 0x003D, [IntPtr]::Zero,
        [IntPtr]$objectId, 0x0002, 2000, [ref]$result)
    Write-Output ("{0}: answered={1} lresult={2}" -f $name, ($ok -ne [IntPtr]::Zero), $result.ToInt64())
}
