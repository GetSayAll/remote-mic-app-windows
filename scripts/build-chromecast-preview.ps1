# Requires -Version 7.0
# 本脚本生成通用 Chrome 听写的本机预览 EXE。运行前在 x64 MSVC Developer PowerShell
# 配好 Windows SDK、Rust 和 pnpm，并用 pnpm install --frozen-lockfile 安装仓库依赖。
# 仅构建和校验文件。预览使用独立配置目录，仍与常规版共用设备访问的单实例锁。
# This script builds the local Chrome dictation preview EXE. Run from an x64
# MSVC Developer PowerShell with the Windows SDK, Rust and pnpm installed.
# Install repo dependencies with pnpm install --frozen-lockfile first.
# It only builds and verifies files. Settings are separate and the hardware
# single-instance lock remains shared with regular SayAll.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# 验证编译命令，并保存调用者环境。锁定仓库 target 路径与 x64，避免拾取其它
# worktree 的旧二进制。构建通道用于日志和 Cargo 重建判断。
# Check the build commands and save the caller's environment. Pin the repository
# target path and x64 to avoid picking up a binary from another worktree.
# The build channel identifies this preview in logs and Cargo's rebuild checks.
foreach ($command in @('git', 'pnpm', 'cargo', 'link.exe')) {
    Get-Command $command -ErrorAction Stop | Out-Null
}
$repo = Split-Path -Parent $PSScriptRoot
$oldChannel = $env:SAYALL_BUILD_CHANNEL
$oldRevision = $env:SAYALL_SOURCE_REVISION
$oldTag = $env:SAYALL_RELEASE_TAG
$oldTarget = $env:CARGO_TARGET_DIR
$oldBuildTarget = $env:CARGO_BUILD_TARGET
Push-Location $repo
try {
    $revision = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $revision -notmatch '^[0-9a-f]{40}$') {
        throw 'Cannot determine the source revision.'
    }
    $sourceStatus = @(& git status --porcelain=v1 --untracked-files=normal)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot determine whether the source checkout is dirty.' }
    $env:SAYALL_BUILD_CHANNEL = 'chromecast-local-test'
    $env:SAYALL_SOURCE_REVISION = $revision
    $env:SAYALL_RELEASE_TAG = ''
    $env:CARGO_TARGET_DIR = Join-Path $repo 'target'
    $env:CARGO_BUILD_TARGET = 'x86_64-pc-windows-msvc'

    # 先生成含前端资源的裸 EXE。覆盖配置不打包安装器、更新或提权助手。
    # 任何构建或嵌入检查失败都先终止，避免交付上次遗留的 EXE。
    # Build a bare EXE with embedded UI assets first. This configuration skips
    # installer, updater artifacts and the elevated helper. Fail before copying
    # if either the build or the asset check fails, so an old EXE isn't delivered.
    & pnpm tauri build --debug --no-bundle --ci --target x86_64-pc-windows-msvc --config src-tauri/tauri.chromecast-preview.conf.json
    if ($LASTEXITCODE -ne 0) { throw 'Chromecast preview build failed.' }
    $rawExe = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/debug/sayall-windows-app.exe'
    & (Join-Path $PSScriptRoot 'verify-frontend-embedded.ps1') -Executable $rawExe -FrontendDirectory (Join-Path $repo 'dist')

    # 校验通过才复制到新目录，旧包可用于人工回退。报告与 EXE 放在同一目录，
    # 记录基础提交、工作区是否有改动、校验值与签名状态。命令不会启动应用。
    # Copy only after verification, into a fresh directory. Previous builds remain
    # available for manual rollback. Store the base revision, dirty flag, digest
    # and signature status beside the EXE. This command does not launch the app.
    $buildId = '{0}-{1}' -f [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmssfff'), $revision.Substring(0, 8)
    $artifactRoot = Join-Path $env:CARGO_TARGET_DIR "chromecast-local-preview/$buildId"
    New-Item -ItemType Directory -Path $artifactRoot | Out-Null
    $exe = Join-Path $artifactRoot 'SayAll-Chromecast-Windows-x64.exe'
    Copy-Item -LiteralPath $rawExe -Destination $exe
    $report = [ordered]@{
        sourceRevision = $revision
        dirty = ($sourceStatus.Count -ne 0)
        buildChannel = $env:SAYALL_BUILD_CHANNEL
        evidenceLevel = 'build'
        sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
        signature = (Get-AuthenticodeSignature -LiteralPath $exe).Status.ToString()
    }
    $report | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $artifactRoot 'build-metadata.json')
    Write-Host "Preview: $exe"
    Write-Host "SHA-256: $($report.sha256)"
}
finally {
    # 成功和失败均还原调用者环境。验证日志保留在当前终端，原预览包不会被覆盖。
    # Restore the caller environment on success or failure. Verification output
    # remains in this terminal and previous previews are kept intact.
    $env:SAYALL_BUILD_CHANNEL = $oldChannel
    $env:SAYALL_SOURCE_REVISION = $oldRevision
    $env:SAYALL_RELEASE_TAG = $oldTag
    $env:CARGO_TARGET_DIR = $oldTarget
    $env:CARGO_BUILD_TARGET = $oldBuildTarget
    Pop-Location
}
