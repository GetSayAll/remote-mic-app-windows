# RC003 助手区本地快速验证（无需真机 / 提权 / frida）：
#   1) agent 逻辑台：vm 挂具直跑真实 agent 脚本（gate 门内延迟 / targets / 合成 / 清键 / 代次）
#   2) 助手 cargo test（路由、协议、桥接行解析、代次一致性自检）
#   3) fmt 检查（workspace + 助手独立清单）
#   4) -Full：sayall-windows 测试 + runtime-simulation 编译检查（改到 app 侧代码时用）
#
# 用法：
#   powershell -File scripts\verify-rc003-helper.ps1
#   powershell -File scripts\verify-rc003-helper.ps1 -Full
param([switch]$Full)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# ---- MSVC 环境（cargo 需要 link.exe；缺失时用 vswhere 导入 vcvars64） ----
function Import-MsvcEnvironment {
    if (Get-Command link.exe -ErrorAction SilentlyContinue) { return }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vsPath) {
            $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path $vcvars) {
                $dump = cmd /c "`"$vcvars`" >nul 2>&1 && set"
                foreach ($line in $dump) {
                    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
                }
                return
            }
        }
    }
    throw "找不到 link.exe，且无法通过 vswhere 导入 VS 构建环境：请改用 VS Developer PowerShell 运行本脚本"
}
Import-MsvcEnvironment

$failures = @()
function Invoke-Checked([string]$Name, [scriptblock]$Action) {
    Write-Host ("== " + $Name + " ==")
    & $Action
    if ($LASTEXITCODE -ne 0) {
        Write-Host ("  FAIL（exit=" + $LASTEXITCODE + "）") -ForegroundColor Red
        $script:failures += $Name
    } else {
        Write-Host "  OK"
    }
}

Invoke-Checked "agent 逻辑台（vm 挂具）" { & node (Join-Path $repo "hardware\RC003\helper\agent\agent_logic_test.mjs") }
Invoke-Checked "助手 cargo test" { & cargo test --manifest-path (Join-Path $repo "hardware\RC003\helper\Cargo.toml") }
Invoke-Checked "助手 fmt" { & cargo fmt --manifest-path (Join-Path $repo "hardware\RC003\helper\Cargo.toml") -- --check }
Invoke-Checked "workspace fmt" { & cargo fmt --all -- --check }
if ($Full) {
    Invoke-Checked "sayall-windows 测试" { & cargo test -p sayall-windows }
    Invoke-Checked "runtime-simulation 编译检查" { & cargo check -p sayall-windows-app --features runtime-simulation }
}

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host ("FAILED：" + ($failures -join "; ")) -ForegroundColor Red
    exit 1
}
Write-Host "PASS：RC003 助手区验证全部通过"
exit 0
