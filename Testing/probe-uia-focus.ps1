# 聚焦输入框可行性探针（一次性诊断工具，不进入产品、不随安装包发布）。
#
# 用途：对指定进程读取「当前焦点元素」和「可编辑候选」的语义摘要；可选地激活窗口
# 并做一次 SetFocus + 读回验证。用于判断 UIA 路线在目标应用上是否可行、Chromium
# 的按需建树需要多久。
#
# 隐私：只输出控件语义与几何（控制类型 / ClassName / AutomationId / 是否密码 /
# 是否只读 / 矩形），**不输出任何用户文本内容**（Name 只报是否存在与长度）。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessName notepad
#   powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessId 1234 -Json
#   powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessName chrome -Activate -SetFocus
#
# 默认不激活目标窗口（只读探测，不打扰用户）；-SetFocus 建议同时给 -Activate。
#
# 注意：本脚本必须先把自身设为 Per-Monitor V2 DPI 感知，否则 BoundingRectangle
# 会被 DWM 虚拟化，125%/150% 缩放下读到的矩形是错的。

param(
    [string]$ProcessName,
    [int]$ProcessId = 0,
    [string]$WindowTitleContains,
    # 直接指定顶层窗口句柄（十进制或 0x 十六进制）。用于 MainWindowHandle 不是
    # 真实 UI 窗口的应用（实测：微信 4.0 与 WorkBuddy 只有一个 Proxy 窗口）。
    # 注意变量名不能叫 $hwnd：PowerShell 大小写不敏感，会与内部句柄变量撞名并被
    # [string] 类型约束把句柄转成字符串。
    [string]$WindowHandle,
    # 列出匹配进程的所有顶层窗口后退出（用于找出真实 UI 窗口）。
    [switch]$ListWindows,
    # 列出目标窗口的全部子窗口（找 Chromium 的 render widget host 用）。
    [switch]$ListChildren,
    # 对目标窗口与其所有子窗口发送 WM_GETOBJECT(UiaRootObjectId)，唤醒按需构建的
    # 无障碍树（Chromium/Electron 的公开机制），随后再扫描。
    [switch]$Wake,
    [switch]$Activate,
    [switch]$SetFocus,
    [switch]$Json,
    [int]$Attempts = 3,
    [int]$AttemptGapMs = 250,
    [int]$MaxCandidates = 40,
    # 宽口径扫描：不只看 Edit/Document，而是统计整棵树的控制类型，并收集
    # 「有 TextPattern」与「可聚焦」的元素——用于判断自绘应用（如微信）把输入框
    # 暴露成了什么，或者根本没有暴露。
    [switch]$Broad,
    [int]$MaxScan = 4000,
    [int]$MaxListed = 40
)

$ErrorActionPreference = 'Stop'

# --- DPI 感知：脚本自身是普通 PowerShell 进程，默认不感知 DPI ---
Add-Type -Namespace SayAllProbe -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true)]
public static extern bool SetProcessDpiAwarenessContext(System.IntPtr value);
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true)]
public static extern bool SetProcessDPIAware();
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool SetForegroundWindow(System.IntPtr hWnd);
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern System.IntPtr GetForegroundWindow();
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool ShowWindow(System.IntPtr hWnd, int nCmdShow);
'@

$dpiNote = 'unknown'
try {
    if ([SayAllProbe.Native]::SetProcessDpiAwarenessContext([IntPtr](-4))) {
        $dpiNote = 'per_monitor_v2'
    } elseif ([SayAllProbe.Native]::SetProcessDPIAware()) {
        $dpiNote = 'system_aware_fallback'
    } else {
        $dpiNote = 'unchanged'
    }
} catch {
    $dpiNote = 'error: ' + $_.Exception.Message
}

# --- 顶层窗口枚举（找真实 UI 窗口用；MainWindowHandle 对部分应用是 Proxy 窗口） ---
Add-Type -Namespace SayAllProbe -Name Windows -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll")]
private static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, System.IntPtr lParam);
private delegate bool EnumWindowsProc(System.IntPtr hWnd, System.IntPtr lParam);
[System.Runtime.InteropServices.DllImport("user32.dll")]
private static extern uint GetWindowThreadProcessId(System.IntPtr hWnd, out uint pid);
[System.Runtime.InteropServices.DllImport("user32.dll")]
private static extern bool IsWindowVisible(System.IntPtr hWnd);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
private static extern int GetWindowText(System.IntPtr hWnd, System.Text.StringBuilder text, int max);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
private static extern int GetClassName(System.IntPtr hWnd, System.Text.StringBuilder text, int max);
[System.Runtime.InteropServices.DllImport("user32.dll")]
private static extern bool GetWindowRect(System.IntPtr hWnd, out RECT rect);
[System.Runtime.InteropServices.DllImport("user32.dll")]
private static extern bool IsIconic(System.IntPtr hWnd);
[System.Runtime.InteropServices.DllImport("dwmapi.dll")]
private static extern int DwmGetWindowAttribute(System.IntPtr hWnd, int attribute, out int value, int size);
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
private struct RECT { public int Left, Top, Right, Bottom; }

public static bool IsMinimized(System.IntPtr hWnd) { return IsIconic(hWnd); }

public static bool IsCloaked(System.IntPtr hWnd) {
    const int DWMWA_CLOAKED = 14;
    int value;
    int hr = DwmGetWindowAttribute(hWnd, DWMWA_CLOAKED, out value, sizeof(int));
    return hr == 0 && value != 0;
}

[System.Runtime.InteropServices.DllImport("user32.dll")]
private static extern bool EnumChildWindows(System.IntPtr parent, EnumWindowsProc callback, System.IntPtr lParam);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Auto)]
private static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint msg, System.IntPtr wParam,
    System.IntPtr lParam, uint flags, uint timeout, out System.IntPtr result);

// UiaRootObjectId = -25（UIA 客户端向窗口要 UIA provider 的 object id）。
public static int WakeUia(System.IntPtr hWnd) {
    const uint WM_GETOBJECT = 0x003D;
    const uint SMTO_ABORTIFHUNG = 0x0002;
    System.IntPtr result;
    System.IntPtr ok = SendMessageTimeout(hWnd, WM_GETOBJECT, System.IntPtr.Zero,
        new System.IntPtr(-25), SMTO_ABORTIFHUNG, 2000, out result);
    return ok == System.IntPtr.Zero ? 0 : 1;
}

public static string[] DumpChildren(System.IntPtr parent) {
    var list = new System.Collections.Generic.List<string>();
    EnumChildWindows(parent, delegate(System.IntPtr hWnd, System.IntPtr lParam) {
        var cls = new System.Text.StringBuilder(256);
        GetClassName(hWnd, cls, 256);
        RECT rect;
        GetWindowRect(hWnd, out rect);
        list.Add(string.Format("child=0x{0:X}|visible={1}|rect={2},{3} {4}x{5}|class={6}",
            hWnd.ToInt64(), IsWindowVisible(hWnd), rect.Left, rect.Top,
            rect.Right - rect.Left, rect.Bottom - rect.Top, cls));
        return true;
    }, System.IntPtr.Zero);
    return list.ToArray();
}


public static uint PidOf(System.IntPtr hWnd) {
    uint pid;
    GetWindowThreadProcessId(hWnd, out pid);
    return pid;
}

public static string[] Dump(uint filterPid) {
    var list = new System.Collections.Generic.List<string>();
    EnumWindows(delegate(System.IntPtr hWnd, System.IntPtr lParam) {
        uint pid;
        GetWindowThreadProcessId(hWnd, out pid);
        if (filterPid != 0 && pid != filterPid) return true;
        var title = new System.Text.StringBuilder(256);
        GetWindowText(hWnd, title, 256);
        var cls = new System.Text.StringBuilder(256);
        GetClassName(hWnd, cls, 256);
        RECT rect;
        GetWindowRect(hWnd, out rect);
        list.Add(string.Format("hwnd=0x{0:X}|pid={1}|visible={2}|minimized={3}|cloaked={4}|rect={5},{6} {7}x{8}|class={9}|title={10}",
            hWnd.ToInt64(), pid, IsWindowVisible(hWnd), IsMinimized(hWnd), IsCloaked(hWnd), rect.Left, rect.Top,
            rect.Right - rect.Left, rect.Bottom - rect.Top, cls, title));
        return true;
    }, System.IntPtr.Zero);
    return list.ToArray();
}
'@

function Get-TargetProcesses {
    if ($ProcessId -gt 0) {
        return @(Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)
    }
    if (-not $ProcessName) { throw 'need -ProcessName or -ProcessId' }
    return @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
}

if ($ListWindows) {
    $pids = @(Get-TargetProcesses | ForEach-Object { $_.Id })
    $rows = @()
    foreach ($row in [SayAllProbe.Windows]::Dump(0)) {
        $parts = $row -split '\|'
        $rowPid = [int](($parts[1] -split '=')[1])
        if ($pids -contains $rowPid) { $rows += $row }
    }
    if ($Json) {
        @{ ok = $true; pids = $pids; windows = $rows } | ConvertTo-Json -Depth 6
    } else {
        Write-Output ("pids=" + ($pids -join ','))
        $rows | ForEach-Object { Write-Output $_ }
    }
    exit 0
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function Get-Safe([scriptblock]$Getter) {
    try { return & $Getter } catch { return $null }
}

function Get-RectText($rect) {
    if (-not $rect) { return 'n/a' }
    $x = [double]$rect.X
    # 最小化 / 离屏窗口的 BoundingRectangle 可能是非有限值（±Infinity 或 NaN），
    # 直接转 int 会抛异常；这是产品代码同样必须处理的边界。
    if ([double]::IsNaN($x) -or [double]::IsInfinity($x)) { return 'nonfinite' }
    $y = [double]$rect.Y
    $w = [double]$rect.Width
    $h = [double]$rect.Height
    if ([double]::IsNaN($y) -or [double]::IsInfinity($y) -or
        [double]::IsNaN($w) -or [double]::IsInfinity($w) -or
        [double]::IsNaN($h) -or [double]::IsInfinity($h)) { return 'nonfinite' }
    if ($w -le 0 -or $h -le 0) { return 'empty' }
    return ("{0},{1} {2}x{3}" -f [int]$x, [int]$y, [int]$w, [int]$h)
}

function Get-ElementSummary($element, $index) {
    $readOnly = $null
    try {
        $pattern = $null
        if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $readOnly = $pattern.Current.IsReadOnly
        }
    } catch { $readOnly = $null }

    [pscustomobject]@{
        index          = $index
        control_type   = Get-Safe { $element.Current.ControlType.ProgrammaticName }
        class_name     = Get-Safe { $element.Current.ClassName }
        automation_id  = Get-Safe { $element.Current.AutomationId }
        is_password    = Get-Safe { $element.Current.IsPassword }
        is_enabled     = Get-Safe { $element.Current.IsEnabled }
        focusable      = Get-Safe { $element.Current.IsKeyboardFocusable }
        has_focus      = Get-Safe { $element.Current.HasKeyboardFocus }
        offscreen      = Get-Safe { $element.Current.IsOffscreen }
        value_readonly = $readOnly
        name_len       = Get-Safe { if ($element.Current.Name) { $element.Current.Name.Length } else { 0 } }
        rect_text      = Get-RectText (Get-Safe { $element.Current.BoundingRectangle })
    }
}

# --- 定位目标进程与窗口 ---
if ($WindowHandle) {
    $raw = $WindowHandle.Trim()
    if ($raw.StartsWith('0x') -or $raw.StartsWith('0X')) {
        $handleValue = [Convert]::ToInt64($raw.Substring(2), 16)
    } else {
        $handleValue = [Convert]::ToInt64($raw)
    }
    $hwnd = [IntPtr]$handleValue
    $targetPid = [SayAllProbe.Windows]::PidOf($hwnd)
    $proc = Get-Process -Id $targetPid -ErrorAction SilentlyContinue
    if (-not $proc) {
        $msg = "no process for hwnd=$Hwnd"
        if ($Json) { @{ ok = $false; error = $msg } | ConvertTo-Json -Depth 6 } else { Write-Output "ERROR: $msg" }
        exit 2
    }
} else {
    $found = @(Get-TargetProcesses)
    $found = @($found | Where-Object { $_.MainWindowHandle -ne 0 })
    if ($WindowTitleContains) {
        $found = @($found | Where-Object { $_.MainWindowTitle -and $_.MainWindowTitle.Contains($WindowTitleContains) })
    }
    if ($found.Count -eq 0) {
        $msg = "no process with a main window matches (name=$ProcessName id=$ProcessId title~$WindowTitleContains)"
        if ($Json) { @{ ok = $false; error = $msg } | ConvertTo-Json -Depth 6 } else { Write-Output "ERROR: $msg" }
        exit 2
    }
    $proc = $found[0]
    $hwnd = [IntPtr]$proc.MainWindowHandle
}

$wasForeground = ([SayAllProbe.Native]::GetForegroundWindow() -eq $hwnd)
if ($Activate) {
    [void][SayAllProbe.Native]::ShowWindow($hwnd, 9)   # SW_RESTORE
    [void][SayAllProbe.Native]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 250
}
$isForeground = ([SayAllProbe.Native]::GetForegroundWindow() -eq $hwnd)
$minimized = [SayAllProbe.Windows]::IsMinimized($hwnd)
$cloaked = [SayAllProbe.Windows]::IsCloaked($hwnd)

$childWindows = @([SayAllProbe.Windows]::DumpChildren($hwnd))
if ($ListChildren) {
    Write-Output ("target hwnd=0x{0:X} class-children={1}" -f $hwnd.ToInt64(), $childWindows.Count)
    $childWindows | ForEach-Object { Write-Output $_ }
    exit 0
}

$wakeResult = $null
if ($Wake) {
    $answered = 0
    $attempted = 0
    foreach ($line in @("target") + $childWindows) {
        $handle = if ($line -eq "target") { $hwnd } else { [IntPtr][Convert]::ToInt64((($line -split '\|')[0] -split '=')[1].Substring(2), 16) }
        $attempted++
        if ([SayAllProbe.Windows]::WakeUia($handle) -eq 1) { $answered++ }
    }
    $wakeResult = @{ attempted = $attempted; answered = $answered }
    Start-Sleep -Milliseconds 400
}

# --- 当前焦点元素 ---
$focusedInfo = $null
try {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($focused) {
        $fpid = Get-Safe { $focused.Current.ProcessId }
        $focusedInfo = Get-ElementSummary $focused -1
        $focusedInfo | Add-Member -NotePropertyName process_id -NotePropertyValue $fpid
        $focusedInfo | Add-Member -NotePropertyName belongs_to_target -NotePropertyValue ($fpid -eq $proc.Id)
    }
} catch {
    $focusedInfo = @{ error = $_.Exception.Message }
}

# --- 候选扫描（重复多次，观察 Chromium/Electron 的按需建树） ---
$root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
$editCondition = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Edit)
$documentCondition = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Document)
$condition = New-Object System.Windows.Automation.OrCondition($editCondition, $documentCondition)

$attemptsResult = @()
$candidates = $null
for ($i = 1; $i -le $Attempts; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $errorText = $null
    $foundHere = $null
    try {
        $foundHere = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    } catch {
        $errorText = $_.Exception.Message
    }
    $sw.Stop()
    $count = if ($foundHere) { $foundHere.Count } else { 0 }
    $attemptsResult += [pscustomobject]@{ attempt = $i; count = $count; elapsed_ms = [int]$sw.ElapsedMilliseconds; error = $errorText }
    if ($count -gt 0) { $candidates = $foundHere }
    if ($i -lt $Attempts) { Start-Sleep -Milliseconds $AttemptGapMs }
}

$candidateSummaries = @()
if ($candidates) {
    $limit = [Math]::Min($candidates.Count, $MaxCandidates)
    for ($i = 0; $i -lt $limit; $i++) {
        $candidateSummaries += Get-ElementSummary $candidates[$i] $i
    }
}

# --- 可选：SetFocus + 读回验证 ---
$setFocusResult = $null
if ($SetFocus) {
    if (-not $candidates -or $candidates.Count -eq 0) {
        $setFocusResult = @{ result = 'no_candidate' }
    } else {
        $chosen = $null
        $chosenIndex = -1
        for ($i = 0; $i -lt $candidates.Count; $i++) {
            $summary = $candidateSummaries | Where-Object { $_.index -eq $i }
            if ($summary -and $summary.is_enabled -eq $true -and $summary.focusable -eq $true -and $summary.is_password -ne $true) {
                $chosen = $candidates[$i]
                $chosenIndex = $i
                break
            }
        }
        if (-not $chosen) {
            $setFocusResult = @{ result = 'no_eligible_candidate' }
        } else {
            $beforeId = Get-Safe { ($chosen.GetRuntimeId() -join '.') }
            $errorText = $null
            try { $chosen.SetFocus() } catch { $errorText = $_.Exception.Message }
            Start-Sleep -Milliseconds 300
            $after = $null
            try { $after = [System.Windows.Automation.AutomationElement]::FocusedElement } catch { }
            $afterId = if ($after) { Get-Safe { ($after.GetRuntimeId() -join '.') } } else { $null }
            $setFocusResult = @{
                result             = if ($errorText) { 'error' } else { 'called' }
                error              = $errorText
                chosen_index       = $chosenIndex
                readback_same      = ($afterId -eq $beforeId)
                readback_has_focus = if ($after) { Get-Safe { $after.Current.HasKeyboardFocus } } else { $null }
            }
        }
    }
}

# --- 宽口径扫描（-Broad）：整棵树的控制类型直方图 + TextPattern / 可聚焦元素 ---
$broadResult = $null
if ($Broad) {
    $all = $null
    $broadError = $null
    $broadSw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $all = $root.FindAll(
            [System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.Condition]::TrueCondition)
    } catch {
        $broadError = $_.Exception.Message
    }
    $broadSw.Stop()

    $histogram = @{}
    $textElements = @()
    $focusableElements = @()
    $textCount = 0
    $focusableCount = 0
    $total = if ($all) { $all.Count } else { 0 }
    $limit = [Math]::Min($total, $MaxScan)
    for ($i = 0; $i -lt $limit; $i++) {
        $el = $all[$i]
        $ctl = Get-Safe { $el.Current.ControlType.ProgrammaticName }
        if (-not $ctl) { $ctl = '<unknown>' }
        if ($histogram.ContainsKey($ctl)) { $histogram[$ctl]++ } else { $histogram[$ctl] = 1 }
        $hasText = Get-Safe { $el.GetCurrentPropertyValue([System.Windows.Automation.AutomationElement]::IsTextPatternAvailableProperty) }
        if ($hasText -eq $true) {
            $textCount++
            if ($textElements.Count -lt $MaxListed) { $textElements += (Get-ElementSummary $el $i) }
        }
        if ((Get-Safe { $el.Current.IsKeyboardFocusable }) -eq $true) {
            $focusableCount++
            if ($focusableElements.Count -lt $MaxListed) { $focusableElements += (Get-ElementSummary $el $i) }
        }
    }
    $broadResult = [pscustomobject]@{
        total_found           = $total
        scanned               = $limit
        truncated             = ($total -gt $limit)
        elapsed_ms            = [int]$broadSw.ElapsedMilliseconds
        error                 = $broadError
        control_types         = $histogram
        text_pattern_count    = $textCount
        text_pattern_elements = $textElements
        focusable_count       = $focusableCount
        focusable_elements    = $focusableElements
    }
}

$report = [pscustomobject]@{
    ok               = $true
    process_name     = $proc.ProcessName
    process_id       = $proc.Id
    window_title_len = if ($proc.MainWindowTitle) { $proc.MainWindowTitle.Length } else { 0 }
    dpi_awareness    = $dpiNote
    was_foreground   = $wasForeground
    foreground       = $isForeground
    minimized        = $minimized
    cloaked          = $cloaked
    child_windows    = $childWindows
    wake             = $wakeResult
    focused          = $focusedInfo
    scan_attempts    = $attemptsResult
    candidates       = $candidateSummaries
    set_focus        = $setFocusResult
    broad            = $broadResult
}

if ($Json) {
    $report | ConvertTo-Json -Depth 8
} else {
    Write-Output ("== probe {0} (pid {1}) dpi={2} was_foreground={3} foreground={4}" -f $report.process_name, $report.process_id, $report.dpi_awareness, $report.was_foreground, $report.foreground)
    if ($focusedInfo) {
        Write-Output ("focused: ctl={0} class={1} pid={2} belongs={3} has_focus={4} rect={5}" -f `
            $focusedInfo.control_type, $focusedInfo.class_name, $focusedInfo.process_id, $focusedInfo.belongs_to_target, $focusedInfo.has_focus, $focusedInfo.rect_text)
    } else {
        Write-Output 'focused: <unavailable>'
    }
    foreach ($attempt in $attemptsResult) {
        Write-Output ("scan attempt={0} count={1} elapsed_ms={2} error={3}" -f $attempt.attempt, $attempt.count, $attempt.elapsed_ms, $attempt.error)
    }
    foreach ($candidate in $candidateSummaries) {
        Write-Output ("cand[{0}] ctl={1} class={2} autoid='{3}' pwd={4} en={5} focusable={6} hasfocus={7} readonly={8} name_len={9} rect={10}" -f `
            $candidate.index, $candidate.control_type, $candidate.class_name, $candidate.automation_id, $candidate.is_password, `
            $candidate.is_enabled, $candidate.focusable, $candidate.has_focus, $candidate.value_readonly, $candidate.name_len, $candidate.rect_text)
    }
    if ($setFocusResult) { Write-Output ("set_focus: " + ($setFocusResult | ConvertTo-Json -Compress)) }
}
