param([int]$TimeoutMs = 45000, [string]$CleanupHelperPath, [switch]$SelfTest)
$ErrorActionPreference = 'Stop'

function Test-CleanupInteger($Value) {
    return $Value -is [int] -or $Value -is [long] -or $Value -is [uint32] -or $Value -is [uint64]
}

function Test-CaptureCleanupState($State, [int]$ExpectedPid, [long]$ExpectedStarted, [long]$ProcessStarted = 0) {
    if ($null -eq $State -or $State.status -notin @('passed', 'agent_version_blocked', 'host_exited', 'not_started')) { return $false }
    foreach ($name in @('helper_pid', 'started_unix_ms', 'completed_unix_ms')) {
        if (-not (Test-CleanupInteger $State.$name) -or $State.$name -le 0) { return $false }
    }
    if ($State.helper_pid -gt [uint32]::MaxValue) { return $false }
    if ($State.completed_unix_ms -lt $State.started_unix_ms) { return $false }
    if ($ExpectedPid -gt 0 -and $State.helper_pid -ne $ExpectedPid) { return $false }
    if ($ExpectedStarted -gt 0 -and $State.started_unix_ms -ne $ExpectedStarted) { return $false }
    if ($State.started_unix_ms -lt $ProcessStarted) { return $false }
    if ($State.status -eq 'host_exited') {
        if (-not (Test-CleanupInteger $State.host_pid) -or $State.host_pid -le 0 -or $State.host_pid -gt [uint32]::MaxValue) { return $false }
        if (-not (Test-CleanupInteger $State.host_created) -or $State.host_created -le 0) { return $false }
    }
    return $true
}

function Request-CaptureStop {
    $directory = Join-Path $env:LOCALAPPDATA 'SayAll'
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    # Persist cancellation even if a previously submitted task has not started yet.
    [IO.File]::WriteAllText((Join-Path $directory 'rc003-capture-stop'), 'stop')
}

function Get-CaptureHelpers {
    $session = (Get-Process -Id $PID).SessionId
    foreach ($process in @(Get-Process -Name 'sayall-helper', 'sayall-hid-host-helper', 'sayall-component-helper' -ErrorAction SilentlyContinue)) {
        if ($process.SessionId -eq $session) {
            [pscustomobject]@{
                Id = $process.Id
                Started = ([DateTimeOffset]$process.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds()
                IsLegacy = $process.ProcessName -ne 'sayall-helper'
            }
        }
    }
}

function Read-CaptureCleanupState {
    $path = Join-Path $env:LOCALAPPDATA 'SayAll/rc003-capture-cleanup.json'
    if (Test-Path -LiteralPath $path) { Get-Content -LiteralPath $path -Raw | ConvertFrom-Json }
}

function Start-CaptureCleanupHelper([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.File]::Exists($Path)) { throw 'Bundled cleanup Helper unavailable' }
    $process = Start-Process -FilePath $Path -ArgumentList '--cleanup-only' -Verb RunAs -WindowStyle Hidden -PassThru
    try {
        [pscustomobject]@{
            Id = $process.Id
            Started = ([DateTimeOffset]$process.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds()
        }
    } finally { $process.Dispose() }
}

function Wait-CaptureCleanupPoll { Start-Sleep -Milliseconds 100 }
function Get-CaptureCleanupElapsedMilliseconds($Timer) { $Timer.ElapsedMilliseconds }

function Invoke-CaptureCleanup([string]$CleanupHelperPath, [int]$TimeoutMs) {
    $script:CaptureCleanupPhase = 'request_stop'
    Request-CaptureStop
    $script:CaptureCleanupPhase = 'waiting_existing'
    $expectedPid = 0
    $expectedStarted = 0L
    $processStarted = 0L
    $recoveryAttempted = $false
    # Helper shares one 30 s connection/HELLO deadline. Allow 45 s per Helper
    # for that handshake plus stop delivery, response and normal process exit.
    # A held key can take longer: its 3 s unconfirmed report is NOT an end time.
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $phaseStarted = Get-CaptureCleanupElapsedMilliseconds $timer
    do {
        $running = @(Get-CaptureHelpers)
        if ($running.Count -gt 1) { throw 'Multiple capture Helpers are running' }
        if ($running.Count -eq 1) {
            if ($running[0].IsLegacy) { throw 'Retired capture Helper is still running' }
            if ($expectedPid -gt 0 -and $expectedPid -ne $running[0].Id) { throw 'Helper changed during cleanup' }
            if ($processStarted -gt 0 -and $processStarted -ne $running[0].Started) { throw 'Helper process object changed during cleanup' }
            $expectedPid = $running[0].Id
            $processStarted = $running[0].Started
        }
        $state = Read-CaptureCleanupState
        if ($null -ne $state -and $expectedPid -gt 0 -and $state.helper_pid -eq $expectedPid -and
            $expectedStarted -eq 0 -and (Test-CleanupInteger $state.started_unix_ms) -and $state.started_unix_ms -ge $processStarted) {
            $expectedStarted = $state.started_unix_ms
        }
        if ($running.Count -eq 0) {
            if ($null -eq $state -and $expectedPid -eq 0) { return }
            if (Test-CaptureCleanupState $state $expectedPid $expectedStarted $processStarted) { return }
            if (-not $recoveryAttempted) {
                # The new bundled Helper can prove a host exited or obtain a fresh
                # same-instance stop ACK. The installer never manufactures proof.
                $recoveryAttempted = $true
                $script:CaptureCleanupPhase = 'start_recovery'
                $recovery = Start-CaptureCleanupHelper $CleanupHelperPath
                $script:CaptureCleanupPhase = 'waiting_recovery'
                $expectedPid = $recovery.Id
                $processStarted = $recovery.Started
                $expectedStarted = 0L
                $phaseStarted = Get-CaptureCleanupElapsedMilliseconds $timer
            }
        }
        Wait-CaptureCleanupPoll
    } while (((Get-CaptureCleanupElapsedMilliseconds $timer) - $phaseStarted) -lt $TimeoutMs)
    # A Helper awaiting a real UP keeps its socket and owns the remaining cleanup.
    # Installer timeout only aborts the installer, never the cleanup process.
    throw 'Capture cleanup not confirmed before installation deadline'
}

if ($SelfTest) {
    $passed = [pscustomobject]@{ helper_pid = 10; started_unix_ms = 100; completed_unix_ms = 200; status = 'passed' }
    if (-not (Test-CaptureCleanupState $passed 10 100)) { throw 'Matching completed cleanup rejected' }
    if (Test-CaptureCleanupState $passed 11 100) { throw 'Stale PID accepted' }
    if (Test-CaptureCleanupState $passed 10 101) { throw 'Stale generation accepted' }
    if (Test-CaptureCleanupState $passed 10 100 101) { throw 'Reused process ID accepted' }
    foreach ($status in @('requested', 'unconfirmed')) {
        $passed.status = $status
        if (Test-CaptureCleanupState $passed 10 100) { throw 'Pending cleanup accepted' }
    }
    foreach ($status in @('agent_version_blocked', 'not_started')) {
        $passed.status = $status
        if (-not (Test-CaptureCleanupState $passed 10 100)) { throw 'Settled noncapturing session rejected' }
    }
    $passed.status = 'host_exited'
    if (Test-CaptureCleanupState $passed 10 100) { throw 'Missing host identity accepted' }
    Write-Output 'cleanup-state: 9 checks passed'
    exit 0
}

try {
    Invoke-CaptureCleanup -CleanupHelperPath $CleanupHelperPath -TimeoutMs $TimeoutMs
    Write-Output 'capture-cleanup: settled; no active capture Helper'
    exit 0
} catch {
    # No personal paths, tokens, device identity or configuration content in output.
    Write-Output ('capture-cleanup: phase=' + $script:CaptureCleanupPhase + ' result=unconfirmed; installation stopped without terminating any process')
    exit 15
}
