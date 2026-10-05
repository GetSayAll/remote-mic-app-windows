# CI 低频守望：结构化输出，避免高频 watch 刷日志/烧 token。
#   -Pr <n>     守望该 PR 的全部检查（gh pr checks）
#   -RunId <id> 守望单个 run（gh run view）
# 每 -IntervalSeconds 查一次、最多 -MaxChecks 次；结论收敛即退出。
#
# 用法：
#   powershell -File scripts\watch-ci-run.ps1 -Pr 179
#   powershell -File scripts\watch-ci-run.ps1 -RunId 37101084835
param(
    [int]$Pr = 0,
    [string]$RunId = "",
    [int]$IntervalSeconds = 540,
    [int]$MaxChecks = 3
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
if ($Pr -eq 0 -and -not $RunId) { throw "至少给一个：-Pr <n> 或 -RunId <id>" }
function Log([string]$m) { Write-Host ((Get-Date -Format "HH:mm:ss") + " " + $m) }

for ($i = 1; $i -le $MaxChecks; $i++) {
    Start-Sleep -Seconds $IntervalSeconds
    Log ("check " + $i + "/" + $MaxChecks)
    if ($RunId) {
        $j = (& gh run view $RunId --json status,conclusion,headSha,displayTitle | Out-String).Trim()
        Write-Host $j
        if ($j -match '"status":"completed"') { break }
    } else {
        $j = (& gh pr checks $Pr --json name,state,link | Out-String).Trim()
        Write-Host $j
        # 全部检查都有明确结论（无排队/进行中）即收敛
        if ($j -notmatch 'IN_PROGRESS|PENDING|QUEUED') { break }
    }
}
Log "DONE"
exit 0
