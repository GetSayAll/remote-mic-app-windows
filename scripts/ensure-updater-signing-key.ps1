# CI 兜底：TAURI_SIGNING_PRIVATE_KEY（GitHub Secret）为空时生成一次性临时
# 更新签名密钥并导出到 GITHUB_ENV，保证未配置 Secret 的 PR / fork 构建仍然
# 绿灯（tauri.conf.json 含 updater pubkey 时，缺私钥的 bundle 构建会直接失败）。
# 临时密钥仅用于让产物构建流程完整走通，其签名与正式 pubkey 不匹配——
# 绝不可用于正式发布；正式发布走 windows-release.yml，缺 Secret 直接失败。
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    Write-Host "使用仓库 Secret 提供的正式更新签名密钥"
    exit 0
}

if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    throw "RUNNER_TEMP 未设置：临时密钥兜底仅支持 CI 环境；本地构建请设置 TAURI_SIGNING_PRIVATE_KEY"
}

$keyPath = Join-Path $env:RUNNER_TEMP "sayall-throwaway-updater.key"
if (Test-Path -LiteralPath $keyPath -PathType Leaf) {
    Remove-Item -LiteralPath $keyPath -Force
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repositoryRoot
try {
    & pnpm tauri signer generate -w $keyPath --password "" --ci
    if ($LASTEXITCODE -ne 0) {
        throw "生成一次性更新签名密钥失败（exit $LASTEXITCODE）"
    }
} finally {
    Pop-Location
}

# TAURI_SIGNING_PRIVATE_KEY 支持私钥文件路径或内容；导出路径供后续构建步骤使用。
Add-Content -Encoding UTF8 -LiteralPath $env:GITHUB_ENV "TAURI_SIGNING_PRIVATE_KEY=$keyPath"
Write-Host "已生成一次性临时更新签名密钥（与正式 pubkey 不匹配，仅用于构建流程绿灯）：$keyPath"
