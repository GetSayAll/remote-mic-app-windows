# Step 5: rotate old diag log, start app WITH SAYALL_GATT_LOG, wait, poll UI
# ASCII only.
$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# rotate old log (keep history; do not delete per repo rule)
$old = "$env:USERPROFILE\sayall-diag.log"
if (Test-Path $old) {
    Move-Item $old "$env:USERPROFILE\sayall-diag-20260906.log" -Force
    Write-Output 'OLD_LOG_ROTATED'
}

$exe = $null
foreach ($d in Get-ChildItem $env:LOCALAPPDATA -Directory) {
    if ($d.Name -like '*SayAll*') {
        $cand = Get-ChildItem $d.FullName -Filter 'sayall-windows-app.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($cand) { $exe = $cand.FullName; break }
    }
}
if (-not $exe) { Write-Output 'EXE_NOT_FOUND'; exit 1 }
Write-Output "EXE=$exe"

$env:SAYALL_GATT_LOG = "$env:USERPROFILE\sayall-diag.log"
Start-Process -FilePath $exe
Write-Output 'APP_STARTED_WITH_DIAG'

# wait for process + first reconnect cycle
Start-Sleep -Seconds 20
$proc = Get-Process -Name 'sayall-windows-app'
if (-not $proc) { Write-Output 'APP_NOT_RUNNING_AFTER_START'; exit 2 }
Write-Output "RUNNING PID=$($proc.Id)"

if (Test-Path "$env:USERPROFILE\sayall-diag.log") {
    $sz = (Get-Item "$env:USERPROFILE\sayall-diag.log").Length
    Write-Output "DIAG_LOG_SIZE=$sz"
} else {
    Write-Output 'DIAG_LOG_NOT_CREATED_YET'
}

# poll UI phase for up to 60s
$waiting = [char]0x6B63 + [char]0x5728 + [char]0x7B49 + [char]0x5F85
$connected = [char]0x5DF2 + [char]0x8FDE + [char]0x63A5
$ready = [char]0x5C31 + [char]0x7EEA
for ($i = 1; $i -le 10; $i++) {
    Start-Sleep -Seconds 6
    $p2 = Get-Process -Name 'sayall-windows-app'
    if (-not $p2) { Write-Output "T+$($i*6)s APP_GONE"; break }
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($p2.MainWindowHandle)
    if (-not $root) { continue }
    $texts = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $phase = ''; $err = ''
    foreach ($t in $texts) {
        $n = $t.Current.Name
        if (-not $n) { continue }
        if ($n -like "$waiting*" -or $n -eq $connected -or $n -eq $ready) { $phase = $n }
        if ($n -like '*GATT*' -or $n -like '*failed*' -or $n -like '*Unreachable*') { $err = $n }
    }
    Write-Output "T+$($i*6)s PHASE=[$phase] ERR=[$err]"
    if ($phase -eq $connected -or $phase -eq $ready) { Write-Output 'CONNECTED_OK'; break }
}
