$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$source = [IO.File]::ReadAllText((Join-Path $root 'src-tauri/windows/verify-capture-cleanup.ps1'))
$tokens = $null; $parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Cleanup verifier parse failed' }
# Load function declarations only, even when testing an old verifier without an
# import-only entry point. Its top-level installer actions are never evaluated.
foreach ($declaration in $ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.FunctionDefinitionAst] }) {
    . ([scriptblock]::Create($declaration.Extent.Text))
}
$checks = 0
function Assert-Cleanup($condition, [string]$message) { if (-not $condition) { throw $message }; $script:checks++ }
function New-State([string]$status = 'passed', [long]$id = 10, [long]$start = 100) {
    [pscustomobject]@{ helper_pid=$id; started_unix_ms=$start; completed_unix_ms=200; status=$status; host_pid=20; host_created=400; agent_instance='instance' }
}
foreach ($status in @('passed', 'agent_version_blocked', 'host_exited', 'not_started')) {
    $state = New-State $status
    Assert-Cleanup (Test-CaptureCleanupState $state 10 100 90) "Settled $status rejected"
    Assert-Cleanup (-not (Test-CaptureCleanupState $state 11 100 90)) 'Wrong PID accepted'
    Assert-Cleanup (-not (Test-CaptureCleanupState $state 10 101 90)) 'Wrong generation accepted'
    Assert-Cleanup (-not (Test-CaptureCleanupState $state 10 100 101)) 'Reused PID accepted'
}
foreach ($status in @('requested', 'unconfirmed', 'unknown')) { Assert-Cleanup (-not (Test-CaptureCleanupState (New-State $status) 0 0)) 'Unsettled state accepted' }
$state = New-State 'host_exited'; $state.host_created = 0
Assert-Cleanup (-not (Test-CaptureCleanupState $state 0 0)) 'Host-exited without object identity accepted'
$state = New-State; $state.helper_pid = '10'
Assert-Cleanup (-not (Test-CaptureCleanupState $state 0 0)) 'Malformed numeric receipt accepted'
$state = New-State; $state.completed_unix_ms = 99
Assert-Cleanup (-not (Test-CaptureCleanupState $state 0 0)) 'Completion before start accepted'
$oldTerminal = [pscustomobject]@{helper_pid=10; started_unix_ms=100; completed_unix_ms=200; status='passed'}
Assert-Cleanup (Test-CaptureCleanupState $oldTerminal 0 0) 'Existing genuine terminal receipt rejected'

# Mock only the OS boundaries. The real coordinator runs unchanged and no actual
# LOCALAPPDATA file, process, named pipe, task or capture endpoint is touched.
function Request-CaptureStop { $script:stopCalls++ }
function Get-CaptureHelpers { @($script:steps[$script:step].running) }
function Read-CaptureCleanupState { $script:steps[$script:step].state }
function Start-CaptureCleanupHelper($Path) { $script:startCalls++; [pscustomobject]@{Id=30; Started=150} }
function Get-CaptureCleanupElapsedMilliseconds($Timer) { $script:elapsed }
function Wait-CaptureCleanupPoll {
    if ($script:step -ge $script:steps.Count-1) { throw 'fixture exhausted without coordinator deadline' }
    $script:step++
    $script:elapsed += $(if ($script:steps[$script:step].ContainsKey('advance')) { $script:steps[$script:step].advance } else { 100 })
}
function Run-Scenario($Steps, [bool]$ExpectPass, [int]$ExpectedStarts, [int]$Timeout=10000, [string]$ExpectedError='') {
    $script:steps=$Steps; $script:step=0; $script:stopCalls=0; $script:startCalls=0; $script:elapsed=0
    $okay=$false
    $reason=''
    try { Invoke-CaptureCleanup -CleanupHelperPath 'fixture-only.exe' -TimeoutMs $Timeout; $okay=$true } catch { $reason=$_.Exception.Message }
    Assert-Cleanup ($okay -eq $ExpectPass) 'Coordinator result differs from expected'
    Assert-Cleanup ($script:startCalls -eq $ExpectedStarts) 'Unexpected parallel/repeated cleanup launch'
    Assert-Cleanup ($script:stopCalls -eq 1) 'Stop intent must persist before any process check'
    if ($ExpectedError) { Assert-Cleanup ($reason -eq $ExpectedError) 'Coordinator did not stop at its own deadline' }
}
$none=@(); $live=@([pscustomobject]@{Id=10; Started=90; IsLegacy=$false})
Run-Scenario @(@{running=$none;state=$null}) $true 0
Run-Scenario @(@{running=$none;state=(New-State)}) $true 0
Run-Scenario @(@{running=$live;state=(New-State 'requested')}, @{running=$none;state=(New-State)}) $true 0
Run-Scenario @(@{running=$none;state=(New-State 'unconfirmed')}, @{running=$none;state=(New-State 'passed' 30 160)}) $true 1
Run-Scenario @(@{running=$none;state=(New-State 'unconfirmed')}, @{running=$none;state=(New-State)}) $false 1
Run-Scenario @(@{running=$live;state=(New-State 'unconfirmed')}) $false 0
Run-Scenario @(@{running=@($live[0],$live[0]);state=(New-State)}) $false 0
Run-Scenario @(@{running=@([pscustomobject]@{Id=20;Started=90;IsLegacy=$true});state=(New-State)}) $false 0
Run-Scenario @(@{running=$live;state=$null}, @{running=@([pscustomobject]@{Id=10;Started=95;IsLegacy=$false});state=(New-State)}, @{running=$none;state=(New-State)}) $false 0
Run-Scenario @(@{running=$live;state=(New-State 'requested')}, @{running=$none;state=(New-State);advance=31000}) $true 0 45000
Run-Scenario @(@{running=$live;state=(New-State 'requested')}, @{running=$none;state=(New-State);advance=44000}) $true 0 45000
Run-Scenario @(@{running=$live;state=(New-State 'requested')}, @{running=$none;state=(New-State 'unconfirmed');advance=44000}, @{running=$none;state=(New-State 'passed' 30 160);advance=44000}) $true 1 45000
Run-Scenario @(@{running=$live;state=(New-State 'unconfirmed')}, @{running=$live;state=(New-State 'unconfirmed');advance=45000}) $false 0 45000 'Capture cleanup not confirmed before installation deadline'
Write-Output "capture-cleanup: $checks checks passed; all coordinator OS boundaries isolated"
