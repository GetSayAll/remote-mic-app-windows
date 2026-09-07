$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms

$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
$buttons = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
Write-Host '--- tray buttons with CJK or IME-ish names ---'
foreach ($b in $buttons) {
    $n = $b.Current.Name
    if ($n -and ($n -match '[\u4e00-\u9fff]' -or $n -match 'IME')) {
        Write-Host ("BUTTON: " + $n)
    }
}
Write-Host '--- registered TSF TIPs with 0x0804 profile ---'
$tips = Get-ChildItem "HKLM:\SOFTWARE\Microsoft\CTF\TIP" -ErrorAction SilentlyContinue
foreach ($tip in $tips) {
    $clsid = $tip.PSChildName
    $lang = Get-ChildItem $tip.PSPath -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -eq '0x00000804' }
    if ($lang) {
        $desc = (Get-ItemProperty ("Registry::HKEY_CLASSES_ROOT\CLSID\" + $clsid) -ErrorAction SilentlyContinue).'(default)'
        $prof = Get-ChildItem $lang.PSPath -ErrorAction SilentlyContinue | Select-Object -ExpandProperty PSChildName
        Write-Host ("TIP " + $clsid + " desc=" + $desc + " profiles=" + ($prof -join ','))
    }
}
