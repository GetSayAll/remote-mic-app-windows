# 在 SayAll 窗口的 UIA 树中查找包含指定文本的任意元素。
# 用法: .\uia-find.ps1 -Text "测试一次"
param(
    [Parameter(Mandatory = $true)][string]$Text
)

Add-Type -AssemblyName UIAutomationClient
$desktop = [System.Windows.Automation.AutomationElement]::RootElement
$winCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty, "无线麦 SayAll")
$window = $desktop.FindFirst([System.Windows.Automation.TreeScope]::Children, $winCond)
if (-not $window) { throw "未找到 SayAll 窗口" }

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
