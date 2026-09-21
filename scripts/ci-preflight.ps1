# CI 前置自检：在本地跑通 CI verify 的快速检查步骤，通过后再 push。
#
# 背景（2026-09-05）：CI verify 曾因 rustfmt 格式漂移连续 10 次失败（run
# #64-#73），而 CI 全量流水线约 19 分钟，反馈太慢。本脚本镜像 CI 的快速
# 步骤（默认 7 步，失败时按 CI 步骤名报告）——本地过 = CI 的这些步骤必过
# （同一命令、同一仓库根）。
#
# 2026-09-21 补充：默认加入 runtime-simulation 的**编译检查**（CI 第 7 步的轻量版）。
# 此前默认只跑前 6 步，而 `cargo check --workspace` 不带 feature，覆盖不到
# feature-gated 的 simulation 模块；#98 的 E0063 恰好只在这条路径上暴露，
# 于是"本地 6/6 通过 + CI 红灯"并存了三个提交。
#
# 用法（仓库根目录）：
#   powershell -File scripts\ci-preflight.ps1           # 7 步，含 runtime-simulation 编译检查
#   powershell -File scripts\ci-preflight.ps1 -Full     # 8 步，追加 runtime-simulation Tauri 完整构建（慢，发布前用）
#
# 说明：CI 后续的重型步骤（NSIS 安装包、安装矩阵测试）本地不镜像——
# 它们依赖 CI 环境且极少因常规改动失败；快速步骤通过后 CI 失败的概率
# 已大幅降低。
param(
    [switch]$Full
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# ---- pnpm 解析：优先 PATH 上的 pnpm；退化到 corepack pnpm（免全局安装） ----
$script:UseCorepack = $false
function Test-PnpmAvailable {
    if (Get-Command pnpm -ErrorAction SilentlyContinue) { return $true }
    if (Get-Command corepack -ErrorAction SilentlyContinue) {
        $script:UseCorepack = $true
        return $true
    }
    return $false
}
function Invoke-Pnpm {
    param([string[]]$PnpmArgs)
    if ($script:UseCorepack) {
        & corepack pnpm @PnpmArgs
    } else {
        & pnpm @PnpmArgs
    }
}

$failures = @()
$step = 0

function Invoke-Step {
    param([string]$Name, [scriptblock]$Action)
    $script:step++
    Write-Host ("[$script:step/$(if ($Full) { 8 } else { 7 })] $Name ...")
    & $Action
    if ($LASTEXITCODE -ne 0) {
        Write-Host ("  FAIL（exit=$LASTEXITCODE）——CI 步骤 [$Name] 将失败") -ForegroundColor Red
        $script:failures += $Name
    } else {
        Write-Host "  PASS" -ForegroundColor Green
    }
}

if (-not (Test-PnpmAvailable)) {
    Write-Host "错误：pnpm 与 corepack 均不可用——无法执行前端检查" -ForegroundColor Red
    exit 1
}

# ---- 镜像 CI verify 的步骤（顺序与命令保持一致） ----

Invoke-Step "Install frontend dependencies" {
    Invoke-Pnpm @("install", "--frozen-lockfile")
}

Invoke-Step "Test frontend" {
    Invoke-Pnpm @("test")
}

Invoke-Step "Build frontend" {
    Invoke-Pnpm @("build")
}

Invoke-Step "Check Rust formatting" {
    cargo fmt --all -- --check
}

Invoke-Step "Test Rust workspace" {
    cargo test --workspace
}

Invoke-Step "Check Windows Tauri host" {
    cargo check --workspace
}

# runtime-simulation 是 feature-gated 路径，上面的 `cargo check --workspace` 不带
# feature、覆盖不到它。用轻量 check 补上（依赖已编译时约十几秒，首次会久一些）。
Invoke-Step "Check Windows runtime simulation (compile only)" {
    cargo check -p sayall-windows-app --features runtime-simulation
}

if ($Full) {
    Invoke-Step "Build Windows runtime simulation" {
        $env:VITE_SAYALL_RUNTIME_SIMULATION = "1"
        Invoke-Pnpm @("tauri", "build", "--no-bundle", "--features", "runtime-simulation")
    }
}

# ---- 汇总 ----
Write-Host ""
if ($failures.Count -eq 0) {
    Write-Host ("CI 前置自检全部通过（" + $script:step + " 步）——push 后 CI 的对应步骤应通过。" ) -ForegroundColor Green
    exit 0
} else {
    Write-Host ("CI 前置自检失败 " + $failures.Count + " 项：") -ForegroundColor Red
    foreach ($f in $failures) { Write-Host ("  - " + $f) -ForegroundColor Red }
    Write-Host "修复后再 push（格式问题先 cargo fmt 并独立 style 提交）。"
    exit 1
}
