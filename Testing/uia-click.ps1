# UIA 点击 WebView 内按钮 + 截图（自测用）。
# 用法:
#   .\uia-click.ps1 -Text "连接与语音"          # 点击包含该文本的按钮
#   .\uia-click.ps1 -Text "单击" -Index 2       # 点击第 N 个匹配（跳过前 Index-1 个）
param(
    [Parameter(Mandatory = $true)][string]$Text,
    [int]$Index = 1
)

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Drawing

$root = [System.Windows.Automation.AutomationElement]::FocusedElement
if (-not $root) { $root = [System.Windows.Automation.AutomationElement]::RootElement }
$desktop = [System.Windows.Automation.AutomationElement]::RootElement

# 找到 SayAll 主窗口
$winCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty, "无线麦 SayAll")
$window = $desktop.FindFirst([System.Windows.Automation.TreeScope]::Children, $winCond)
if (-not $window) { throw "未找到 SayAll 窗口" }

$buttonCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Button)
$buttons = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttonCond)

$matches = @()
foreach ($button in $buttons) {
    $name = $button.Current.Name
    if ($name -and $name.Contains($Text)) { $matches += $button }
}
if ($matches.Count -lt $Index) {
    Write-Host "匹配 '$Text' 的按钮只有 $($matches.Count) 个（需要第 $Index 个）；全部按钮名："
    foreach ($button in $buttons) { Write-Host "  - $($button.Current.Name)" }
    throw "目标按钮未找到"
}
$target = $matches[$Index - 1]
try {
    $invoke = $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $invoke.Invoke()
    Write-Host "clicked: $($target.Current.Name)"
} catch {
    throw "无法点击 $($target.Current.Name): $($_.Exception.Message)"
}
