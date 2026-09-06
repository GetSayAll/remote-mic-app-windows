# CI 兜底：TAURI_SIGNING_PRIVATE_KEY（GitHub Secret）为空时生成一次性临时
# 更新签名密钥并导出到 GITHUB_ENV，保证未配置 Secret 的 PR / fork 构建仍然
# 绿灯（tauri.conf.json 含 updater pubkey 时，缺私钥的 bundle 构建会直接失败）。
# 临时密钥仅用于让产物构建流程完整走通，其签名与正式 pubkey 不匹配——
# 绝不可用于正式发布；正式发布走 windows-release.yml，缺 Secret 直接失败。
#
# 实证注记（2026-09-06 本机环境变量矩阵实验，四组合各一次完整构建）：
# - tauri build 的 updater 签名只读 TAURI_SIGNING_PRIVATE_KEY，且要求是密钥
#   **内容**；TAURI_SIGNING_PRIVATE_KEY_PATH 仅是 `signer sign` 子命令的选项，
#   build 不认（只设 _PATH 时报 "A public key has been found, but no private
#   key"）。因此这里按内容形式（GITHUB_ENV heredoc 多行语法）导出。
# - TAURI_SIGNING_PRIVATE_KEY_PASSWORD 缺失时 build 会交互式等待输入密码，
#   无 TTY 的 CI 上直接挂死——必须显式提供（实测：空字符串对无密码密钥有效；
#   本兜底用固定哑密码，全程非空值，无空字符串边界歧义）。
# - `--password ""` 这种独立空参数在 PowerShell → pnpm.CMD → node 的链路上
#   会被丢弃（CI 实证 2026-09-06），必须写成 `--password="..."` 合并 token。
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    Write-Host "使用仓库 Secret 提供的正式更新签名密钥"
    exit 0
}

if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    throw "RUNNER_TEMP 未设置：临时密钥兜底仅支持 CI 环境；本地构建请设置 TAURI_SIGNING_PRIVATE_KEY"
}

$FallbackKeyPassword = "sayall-ci-fallback-throwaway"

$keyPath = Join-Path $env:RUNNER_TEMP "sayall-throwaway-updater.key"
if (Test-Path -LiteralPath $keyPath -PathType Leaf) {
    Remove-Item -LiteralPath $keyPath -Force
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repositoryRoot
try {
    & pnpm tauri signer generate -w $keyPath --password="$FallbackKeyPassword" --ci
    if ($LASTEXITCODE -ne 0) {
        throw "生成一次性更新签名密钥失败（exit $LASTEXITCODE）"
    }
} finally {
    Pop-Location
}

# 按密钥**内容**导出（GITHUB_ENV heredoc 多行语法；不导出文件路径——build 不认
# 路径形式），连同哑密码一并导出，确保后续构建步骤零交互。
$keyContent = (Get-Content -Raw -LiteralPath $keyPath).Trim()
$delimiter = "sayallkey" + [Guid]::NewGuid().ToString('N')
Add-Content -Encoding UTF8 -LiteralPath $env:GITHUB_ENV "TAURI_SIGNING_PRIVATE_KEY<<$delimiter"
Add-Content -Encoding UTF8 -LiteralPath $env:GITHUB_ENV $keyContent
Add-Content -Encoding UTF8 -LiteralPath $env:GITHUB_ENV $delimiter
Add-Content -Encoding UTF8 -LiteralPath $env:GITHUB_ENV "TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$FallbackKeyPassword"
Write-Host "已生成一次性临时更新签名密钥（与正式 pubkey 不匹配，仅用于构建流程绿灯），已按内容形式导出到 GITHUB_ENV"
