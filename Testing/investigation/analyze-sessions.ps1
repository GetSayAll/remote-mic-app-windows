# Analyze voice sessions: v2 with regex (avoid -like bracket wildcard trap)
# ASCII only.
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$log = "$env:USERPROFILE\sayall-diag.log"

$reStart = '^C (\d+) len=\s*4 b=\[04 03 02'
$reStop  = '^C (\d+) len=\s*2 b=\[00 02\]'
$reA     = '^A (\d+) len=120'

$inSession = $false
$sessionStart = 0L
$frames = 0
$lastFrameTs = 0L
$sessionNo = 0
$gapsOver100 = 0
$maxGap = 0
$gapsOver250 = 0
$report = @()
$openStart = 0L
$openFrames = 0

foreach ($line in (Get-Content $log)) {
    if ($line -match $reStart) {
        if ($inSession) { $report += "session#$sessionNo UNTERMINATED frames=$frames" }
        $sessionNo++
        $inSession = $true
        $frames = 0; $gapsOver100 = 0; $gapsOver250 = 0; $maxGap = 0
        $sessionStart = [long]$matches[1]
        $lastFrameTs = $sessionStart
    }
    elseif ($line -match $reA) {
        if ($inSession) {
            $frames++
            $ts = [long]$matches[1]
            if ($ts -gt $lastFrameTs) {
                $gap = $ts - $lastFrameTs
                if ($gap -gt 100) { $gapsOver100++ }
                if ($gap -gt 250) { $gapsOver250++ }
                if ($gap -gt $maxGap) { $maxGap = $gap }
            }
            $lastFrameTs = $ts
        }
    }
    elseif ($line -match $reStop) {
        if ($inSession) {
            $span = $lastFrameTs - $sessionStart
            $audioSec = [math]::Round($frames * 0.015, 2)
            $spanSec = [math]::Round($span / 1000.0, 2)
            $ratio = if ($spanSec -gt 0) { [math]::Round($audioSec / $spanSec * 100, 1) } else { 0 }
            $report += "session#$sessionNo span=${spanSec}s frames=$frames audio=${audioSec}s realtime=${ratio}% gaps>100ms=$gapsOver100 gaps>250ms=$gapsOver250 maxGap=${maxGap}ms"
            $inSession = $false
        }
    }
}
if ($inSession) { $report += "session#$sessionNo STILL_OPEN frames=$frames" }
Write-Output "TOTAL_SESSIONS=$sessionNo"
$report
