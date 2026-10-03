# 主窗口恢复真机探针（2026-10-03）。
#
# 覆盖用户入口：托盘左键点击、关闭到托盘后唤起、二次启动（再次启动同一 exe →
# 单实例守卫 → 命名事件 → 主实例恢复窗口）。判据全部是 Win32 读回（IsIconic /
# IsWindowVisible / GetForegroundWindow），不看任何"命令成功"返回值、不依赖
# 应用自身日志。
#
# 用例 1-4 用托盘回调消息（WM_USER_TRAYICON + WM_LBUTTONUP，即 shell 发给托盘
# 消息窗口的那条消息）；
# 用例 5 用 UI Automation 的 Invoke 走**shell 官方激活路径**（与用户点击同一个
# 系统入口）。
#
# 重要探针坑（2026-10-03 实测）：任务栏通知区域**会过滤注入的鼠标点击**
# （SendInput / mouse_event 都不生效，右键连菜单都不弹）。因此"真实点击"不能
# 用注入实现，必须用 UIA Invoke，或直接发上面那条回调消息。
#
# 用法（先手动启动待测的 sayall-windows-app.exe，再运行）：
#   powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-main-window-restore.ps1
# 指定进程：
#   ... -ProcessId <pid>
# 跳过"二次启动"用例：
#   ... -SkipSecondInstance
#
# 说明：
# - 最小化用 WM_SYSCOMMAND + SC_MINIMIZE（与标题栏/任务栏"最小化"同一条系统路径，
#   tao 的 MINIMIZED 缓存会同步更新）；另测 SW_MINIMIZE 直调路径，用于覆盖缓存与
#   实际状态漂移时的 Win32 兜底恢复。
# - 第二个实例由本脚本用运行中进程的 exe 路径启动；它应在单实例守卫处退出。
# - 用例 5 需要托盘图标显示在任务栏直显区（promoted）；在折叠区时该用例记为
#   skipped（折叠区路径由用例 1-4 的回调消息覆盖）。

param(
    [int]$ProcessId = 0,
    [switch]$SkipSecondInstance
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class SayallWindowProbe {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);

  public static IntPtr MainWindowForPid(uint pid) {
    IntPtr found = IntPtr.Zero; long best = -1;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid) return true;
      if (GetWindowTextLength(h) <= 0) return true;
      if (GetWindow(h, 4) != IntPtr.Zero) return true; // GW_OWNER：排除弹窗/工具窗口
      RECT r;
      if (!GetWindowRect(h, out r)) return true;
      long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
      if (area > best) { best = area; found = h; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  public static IntPtr TrayWindowForPid(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid) return true;
      var sb = new StringBuilder(64);
      GetClassNameW(h, sb, sb.Capacity);
      if (sb.ToString() == "tray_icon_app") { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  [StructLayout(LayoutKind.Sequential)] public struct NID { public int cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NID id, out RECT rect);

  // 托盘图标在 shell 里的 uID 由 tray-icon 的内部计数器分配，这里逐个探测：
  // Shell_NotifyIconGetRect 返回 S_OK 的就是本进程的图标（同时证明图标当前
  // 在通知区域里可见、模拟回调不会被 tray-icon 的 rect 前置检查丢弃）。
  public static uint TrayIconId(IntPtr trayHwnd) {
    for (uint id = 0; id < 32; id++) {
      var nid = new NID();
      nid.cbSize = Marshal.SizeOf(typeof(NID));
      nid.hWnd = trayHwnd;
      nid.uID = id;
      RECT rect;
      if (Shell_NotifyIconGetRect(ref nid, out rect) == 0) { return id; }
    }
    return 0;
  }
}
'@

$WM_SYSCOMMAND = 0x0112
$SC_MINIMIZE = 0xF020
$WM_CLOSE = 0x0010
$WM_USER_TRAYICON = 6002
$WM_LBUTTONUP = 0x0202

function Wait-Condition {
    param([scriptblock]$Condition, [int]$TimeoutMs = 3000)
    $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
    while ($true) {
        if (& $Condition) { return $true }
        if ((Get-Date) -ge $deadline) { return [bool](& $Condition) }
        Start-Sleep -Milliseconds 100
    }
}

function Get-WindowState {
    param([IntPtr]$Hwnd)
    [pscustomobject]@{
        Iconic    = [SayallWindowProbe]::IsIconic($Hwnd)
        Visible   = [SayallWindowProbe]::IsWindowVisible($Hwnd)
        Foreground = ([SayallWindowProbe]::GetForegroundWindow() -eq $Hwnd)
    }
}

function Reset-Window {
    # 每个用例从「可见且非最小化」的已知状态开始：失败的前一个用例可能把窗口
    # 留在最小化/隐藏状态，污染下一个用例的判定。
    [void][SayallWindowProbe]::ShowWindow($hwnd, 9)  # SW_RESTORE
    [void](Wait-Condition -Condition { -not [SayallWindowProbe]::IsIconic($hwnd) -and [SayallWindowProbe]::IsWindowVisible($hwnd) } -TimeoutMs 2000)
    Start-Sleep -Milliseconds 300
}

function Set-WindowMinimized {
    # 真实最小化路径：WM_SYSCOMMAND + SC_MINIMIZE（标题栏/任务栏"最小化"同款，
    # tao 的 MINIMIZED 缓存同步更新）。刚被恢复/置前的窗口偶发需要重试，
    # 这里最多试 3 次，避免用例判定靠运气。
    for ($i = 0; $i -lt 3; $i++) {
        [void][SayallWindowProbe]::PostMessage($hwnd, $WM_SYSCOMMAND, [IntPtr]$SC_MINIMIZE, [IntPtr]::Zero)
        if (Wait-Condition -Condition { [SayallWindowProbe]::IsIconic($hwnd) } -TimeoutMs 900) { return $true }
    }
    return $false
}

if ($ProcessId -le 0) {
    $candidates = @(Get-Process -Name 'sayall-windows-app' -ErrorAction SilentlyContinue)
    if ($candidates.Count -ne 1) {
        throw "找到 $($candidates.Count) 个 sayall-windows-app 进程，请用 -ProcessId 指定"
    }
    $ProcessId = $candidates[0].Id
}

$process = Get-Process -Id $ProcessId
$hwnd = [SayallWindowProbe]::MainWindowForPid([uint32]$ProcessId)
if ($hwnd -eq [IntPtr]::Zero) { throw "进程 $ProcessId 没有找到主窗口" }
$trayHwnd = [SayallWindowProbe]::TrayWindowForPid([uint32]$ProcessId)
$trayIconId = if ($trayHwnd -ne [IntPtr]::Zero) { [SayallWindowProbe]::TrayIconId($trayHwnd) } else { 0 }
$exePath = $process.Path

Write-Host ("[probe] pid={0} hwnd={1} tray_hwnd={2} tray_icon_id={3} exe={4}" -f $ProcessId, $hwnd, $trayHwnd, $trayIconId, $exePath)
if ($trayHwnd -eq [IntPtr]::Zero -or $trayIconId -eq 0) {
    Write-Host "[probe] 警告：托盘图标不可定位（可能未创建或不在通知区域），托盘用例将失败"
}

$results = New-Object System.Collections.Generic.List[object]

function Invoke-TrayClickAndCheck {
    param([string]$Name)
    if ($trayHwnd -eq [IntPtr]::Zero -or $trayIconId -eq 0) {
        $results.Add([pscustomobject]@{ Case = $Name; Passed = $false; Detail = 'tray_icon_app 窗口或图标 uID 未找到' })
        return
    }
    [void][SayallWindowProbe]::PostMessage($trayHwnd, $WM_USER_TRAYICON, [IntPtr]$trayIconId, [IntPtr]$WM_LBUTTONUP)
    $restored = Wait-Condition -Condition { -not [SayallWindowProbe]::IsIconic($hwnd) -and [SayallWindowProbe]::IsWindowVisible($hwnd) } -TimeoutMs 3000
    $state = Get-WindowState -Hwnd $hwnd
    $results.Add([pscustomobject]@{
            Case   = $Name
            Passed = $restored
            Detail = ("iconic={0} visible={1} foreground={2}" -f $state.Iconic, $state.Visible, $state.Foreground)
        })
}

# 用例 1：真实最小化路径（WM_SYSCOMMAND + SC_MINIMIZE，tao 缓存同步更新）→ 托盘左键
Reset-Window
$minimized = Set-WindowMinimized
if (-not $minimized) {
    $results.Add([pscustomobject]@{ Case = '最小化(SC_MINIMIZE)'; Passed = $false; Detail = '窗口未进入最小化，后续用例不可判定' })
    $results | Format-Table -AutoSize
    exit 1
}
Invoke-TrayClickAndCheck -Name '用例1 最小化(SC_MINIMIZE) → 托盘左键'

# 用例 2：SW_MINIMIZE 直调（tao 缓存不更新，覆盖缓存/实际漂移时的 Win32 兜底）→ 托盘左键
Reset-Window
[void][SayallWindowProbe]::ShowWindow($hwnd, 6)  # SW_MINIMIZE
[void](Wait-Condition -Condition { [SayallWindowProbe]::IsIconic($hwnd) } -TimeoutMs 2000)
Invoke-TrayClickAndCheck -Name '用例2 最小化(SW_MINIMIZE) → 托盘左键'

# 用例 3：关闭到托盘（WM_CLOSE → 应用隐藏窗口）→ 托盘左键
Reset-Window
[void][SayallWindowProbe]::PostMessage($hwnd, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
$hidden = Wait-Condition -Condition { -not [SayallWindowProbe]::IsWindowVisible($hwnd) } -TimeoutMs 3000
if ($hidden) {
    Invoke-TrayClickAndCheck -Name '用例3 关闭到托盘 → 托盘左键'
} else {
    $results.Add([pscustomobject]@{ Case = '用例3 关闭到托盘 → 托盘左键'; Passed = $false; Detail = 'WM_CLOSE 后窗口仍未隐藏' })
}

# 用例 4：最小化（SC_MINIMIZE）→ 二次启动（双击快捷方式同一条路径）
if (-not $SkipSecondInstance) {
    Reset-Window
    $minimized = Set-WindowMinimized
    if ($minimized) {
        $second = Start-Process -FilePath $exePath -PassThru
        $secondExited = $second.WaitForExit(15000)
        $restored = Wait-Condition -Condition { -not [SayallWindowProbe]::IsIconic($hwnd) -and [SayallWindowProbe]::IsWindowVisible($hwnd) } -TimeoutMs 3000
        $state = Get-WindowState -Hwnd $hwnd
        $results.Add([pscustomobject]@{
                Case   = '用例4 最小化 → 二次启动'
                Passed = ($secondExited -and $restored)
                Detail = ("second_exited={0} iconic={1} visible={2} foreground={3}" -f $secondExited, $state.Iconic, $state.Visible, $state.Foreground)
            })
    } else {
        $results.Add([pscustomobject]@{ Case = '用例4 最小化 → 二次启动'; Passed = $false; Detail = '窗口未进入最小化，用例不可判定' })
    }
}

# 用例 5：真实任务栏图标激活（UIA Invoke，shell 官方激活路径；与用户点击同一个
# 系统入口）。注入鼠标点击会被通知区域过滤（见文件头），所以这里不用鼠标模拟。
# 图标在任务栏直显区才能找到该 UIA 元素；折叠区时记为 skipped。
Reset-Window
$minimized = Set-WindowMinimized
if (-not $minimized) {
    $results.Add([pscustomobject]@{ Case = '用例5 任务栏图标 UIA 激活'; Passed = $false; Detail = '窗口未进入最小化，用例不可判定' })
} else {
    $uiaReady = $true
    try {
        Add-Type -AssemblyName UIAutomationClient -ErrorAction Stop
        Add-Type -AssemblyName UIAutomationTypes -ErrorAction Stop
    } catch {
        $uiaReady = $false
    }
    $icon = $null
    if ($uiaReady) {
        $root = [System.Windows.Automation.AutomationElement]::RootElement
        $trayBar = $root.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'Shell_TrayWnd')))
        if ($trayBar) {
            $icon = $trayBar.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '无线麦 SayAll')))
        }
    }
    if (-not $uiaReady) {
        $results.Add([pscustomobject]@{ Case = '用例5 任务栏图标 UIA 激活'; Passed = $null; Detail = 'skipped: UI Automation 程序集不可用' })
    } elseif (-not $icon) {
        $results.Add([pscustomobject]@{ Case = '用例5 任务栏图标 UIA 激活'; Passed = $null; Detail = 'skipped: 任务栏未找到图标按钮（图标在折叠区）' })
    } else {
        ($icon.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke()
        $restored = Wait-Condition -Condition { -not [SayallWindowProbe]::IsIconic($hwnd) -and [SayallWindowProbe]::IsWindowVisible($hwnd) } -TimeoutMs 3000
        $state = Get-WindowState -Hwnd $hwnd
        $results.Add([pscustomobject]@{
                Case   = '用例5 任务栏图标 UIA 激活'
                Passed = $restored
                Detail = ("iconic={0} visible={1} foreground={2}" -f $state.Iconic, $state.Visible, $state.Foreground)
            })
    }
}

# 收尾：无论用例结果如何，把窗口恢复到可见（失败用例可能把窗口留在最小化/隐藏，
# 探针不允许把待测应用留在不可用状态）。
Reset-Window

$results | Format-Table -AutoSize
$failed = @($results | Where-Object { $_.Passed -eq $false }).Count
$skipped = @($results | Where-Object { $_.Passed -eq $null }).Count
Write-Host ("[probe] {0}/{1} passed{2}" -f ($results.Count - $failed - $skipped), $results.Count, $(if ($skipped -gt 0) { "（$skipped skipped）" } else { "" }))
if ($failed -gt 0) { exit 1 }
exit 0
