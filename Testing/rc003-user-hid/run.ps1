param(
    [ValidateSet('Probe', 'Inspect', 'Capture', 'Status', 'Stop')][string]$Mode = 'Probe',
    [ValidateSet('frida', 'gadget')][string]$Backend = 'frida',
    [ValidateRange(15, 180)][int]$Seconds = 90
)
$ErrorActionPreference = 'Stop'
$python = Join-Path $PSScriptRoot '.venv\Scripts\python.exe'
$script = Join-Path $PSScriptRoot 'capture.py'
$runs = Join-Path $PSScriptRoot '.runs'
if ($Mode -eq 'Status' -or $Mode -eq 'Stop') {
    $files = @(Get-ChildItem -LiteralPath $runs -Filter '*.jsonl' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTimeUtc -Descending)
    $latest = $files | Select-Object -First 1
    if ($null -eq $latest) { throw 'No diagnostic log exists.' }
    # An inspection or rejected second launch must not hide the active capture.
    $active = @($files | Where-Object {
        try {
            $first = Get-Content -LiteralPath $_.FullName -TotalCount 1 | ConvertFrom-Json
            $last = Get-Content -LiteralPath $_.FullName -Tail 1 | ConvertFrom-Json
            $first.mode -eq 'capture' -and $last.event -notin @('capture_finished', 'failed') -and
                $_.LastWriteTimeUtc -gt [DateTime]::UtcNow.AddMinutes(-4)
        } catch { $false }
    })
    if ($active.Count -gt 0) { $latest = $active[0] }
    if ($Mode -eq 'Stop') {
        if ($active.Count -eq 0) { Write-Output 'No active capture found.'; return }
        foreach ($run in $active) {
            [IO.File]::WriteAllText([IO.Path]::ChangeExtension($run.FullName, '.stop'), 'stop')
        }
        Write-Output 'Stop requested. The capture loop checks this flag; no host process is killed.'
    }
    Get-Content -LiteralPath $latest.FullName -Tail 12
    return
}
if (-not (Test-Path -LiteralPath $python)) {
    throw 'Run prepare.ps1 first. The diagnostic uses an isolated Python environment.'
}
if ($Mode -eq 'Probe') {
    & $python -I -u $script --probe
    if ($LASTEXITCODE -ne 0) { throw 'Preflight failed; no injection was attempted.' }
    return
}
New-Item -ItemType Directory -Path $runs -Force | Out-Null
$name = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
$log = Join-Path $runs ($name + '.jsonl')
$captureMode = if ($Mode -eq 'Inspect') { '--inspect-host' } else { '--capture' }
$arguments = @('-I', '-u', ('"' + $script + '"'), $captureMode, '--backend', $Backend, '--seconds', $Seconds,
    '--log', ('"' + $log + '"'))
# Explicit UAC only. The installed SayAll process is never elevated or restarted.
$process = Start-Process -FilePath $python -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -PassThru
[pscustomobject]@{ HelperPid = $process.Id; Log = $log; CaptureSeconds = $Seconds } | ConvertTo-Json
