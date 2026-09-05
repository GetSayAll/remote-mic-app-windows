# 在指定进程的 SayAll 窗口内查找包含指定文本的元素（多实例并存时按 PID 限定）。
# 用法: .\uia-find-pid.ps1 -Text "语音键" -AppPid 1234
param(
    [Parameter(Mandatory = $true)][string]$Text,
    [Parameter(Mandatory = $true)][int]$AppPid
)

Add-Type -AssemblyName UIAutomationClient
$desktop = [System.Windows.Automation.AutomationElement]::RootElement
$winCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty, "无线麦 SayAll")
$windows = $desktop.FindAll([System.Windows.Automation.TreeScope]::Children, $winCond)
$window = $null
foreach ($w in $windows) {
    if ($w.Current.ProcessId -eq $AppPid) { $window = $w; break }
}
if (-not $window) { throw "未找到 PID=$AppPid 的 SayAll 窗口" }

$elements = $window.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition)
$hits = 0
foreach ($element in $elements) {
    $name = $element.Current.Name
    if ($name -and $name.Contains($Text)) {
        $hits++
        $rect = $element.Current.BoundingRectangle
        Write-Host ("HIT: '{0}' at X={1:N0} Y={2:N0} W={3:N0} H={4:N0}" -f $name.Substring(0, [Math]::Min(40, $name.Length)), $rect.X, $rect.Y, $rect.Width, $rect.Height)
    }
}
Write-Host "total hits: $hits"
