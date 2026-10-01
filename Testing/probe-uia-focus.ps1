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

# --- 定位目标进程与主窗口 ---
if ($ProcessId -gt 0) {
    $found = @(Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)
} else {
    if (-not $ProcessName) { throw 'need -ProcessName or -ProcessId' }
    $found = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
}
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

$wasForeground = ([SayAllProbe.Native]::GetForegroundWindow() -eq $hwnd)
if ($Activate) {
    [void][SayAllProbe.Native]::ShowWindow($hwnd, 9)   # SW_RESTORE
    [void][SayAllProbe.Native]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 250
}
$isForeground = ([SayAllProbe.Native]::GetForegroundWindow() -eq $hwnd)

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
