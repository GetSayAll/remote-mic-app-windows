# 生成应用内更新清单 latest.json 并把 Release 资产复制为纯 ASCII 文件名。
# 用法（Windows Release workflow，tag push 触发）：
#   ./scripts/generate-updater-manifest.ps1 -Tag v0.2.0 [-Repository owner/repo]
# 输出目录 artifacts/windows-release/：
#   latest.json                                # updater 静态清单（tauri.conf.json 版本 + .sig 内容 + 资产直链）
#   SayAll-Windows-<version>-x64-setup.exe     # NSIS 安装器（ASCII 重命名，消除 URL 编码风险）
#   SayAll-Windows-<version>-x64-setup.exe.sig # minisign 签名
#   SHA256SUMS.txt                             # 上述两个二进制资产的校验和
param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$Repository = "GetSayAll/remote-mic-app-windows",
    # 测试/本地运行可显式指定仓库根；CI 默认取脚本所在目录的父目录。
    [string]$RepositoryRoot = "",
    [switch]$RequireWindowsCodeSignature
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = Split-Path -Parent $PSScriptRoot
}
$configPath = Join-Path $RepositoryRoot "src-tauri/tauri.conf.json"
$bundleDirectory = Join-Path $RepositoryRoot "target/release/bundle/nsis"
$stagingDirectory = Join-Path $RepositoryRoot "artifacts/windows-release"

$config = Get-Content -Raw -Encoding UTF8 $configPath | ConvertFrom-Json
$version = [string]$config.version
if ([string]::IsNullOrWhiteSpace($version)) {
    throw "tauri.conf.json 缺少 version，无法生成更新清单"
}

# Tag 与配置版本必须一致：防止用旧 tag 发布新构建或反之（fail fast）。
$tagVersion = $Tag.TrimStart("v")
if ($tagVersion -ne $version) {
    throw "Tag $Tag 与 tauri.conf.json 版本 $version 不一致；版本发布前须先在 main 合入版本号变更"
}

$installers = @(Get-ChildItem -LiteralPath $bundleDirectory -Filter "*-setup.exe" -File)
if ($installers.Count -ne 1) {
    throw "Expected exactly one NSIS installer in $bundleDirectory, found $($installers.Count)"
}
$installer = $installers[0]
if ($installer.Name -notlike "*_$version`_x64-setup.exe") {
    throw "NSIS installer $($installer.Name) does not carry version $version"
}

# 正式候选必须先验证 Windows 代码签名（Authenticode 是 Windows 对发布者和
# EXE 字节的校验），再准备更新资产。输入是同一次构建的主程序和安装器，须在
# 信任发布证书链的 Windows runner 上运行。签名缺失或无效时，此次运行不写资产。
# 修复签名应重新从候选提交构建，随后重新生成更新签名，不能覆盖已发布文件。
# For a production candidate, validate Authenticode (Windows executable signer
# and integrity verification) before writing updater assets. Use the application
# and installer from the same build and a Windows runner that trusts the signing
# chain. Stop before staging on a missing or invalid signature. Rebuild and
# re-sign the updater from the candidate commit, preserving published assets.
if ($RequireWindowsCodeSignature) {
    $application = Join-Path $RepositoryRoot "target/release/sayall-windows-app.exe"
    foreach ($file in @($application, $installer.FullName)) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            throw "Missing production executable: $([IO.Path]::GetFileName($file))"
        }
        $windowsSignature = Get-AuthenticodeSignature -LiteralPath $file
        if ($windowsSignature.Status -ne 'Valid') {
            throw "Invalid Authenticode signature for $([IO.Path]::GetFileName($file)): $($windowsSignature.Status)"
        }
        Write-Host "Verified Authenticode: $([IO.Path]::GetFileName($file))"
    }
}

$signaturePath = "$($installer.FullName).sig"
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
    throw "Updater signature is missing: $signaturePath（构建需要 TAURI_SIGNING_PRIVATE_KEY，且 createUpdaterArtifacts 已开启）"
}
$signature = (Get-Content -Raw -Encoding UTF8 -LiteralPath $signaturePath).Trim()
if ([string]::IsNullOrWhiteSpace($signature)) {
    throw "Updater signature file is empty: $signaturePath"
}

# 正式候选还要验证更新签名确实对应最终安装器，避免 Authenticode 改过文件后
# 误用旧签名。需要可信的 minisign.exe 在 PATH 上。公钥和签名来自 Tauri 配置与
# 构建产物，都是公开信息。失败时不准备资产；临时解码文件无论结果都会删除。
# Verify that the updater signature matches the final installer, preventing a
# stale signature after Authenticode changes the file. Use a trusted minisign.exe
# from PATH and the public updater key in Tauri config. Failure stops staging.
# Always remove the temporary decoded signature.
if ($RequireWindowsCodeSignature) {
    $minisign = Get-Command minisign.exe -CommandType Application -ErrorAction Stop
    $keyText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($config.plugins.updater.pubkey))
    $keyLines = @($keyText -split '\r?\n' | Where-Object { $_ -and $_ -notlike 'untrusted comment:*' })
    if ($keyLines.Count -ne 1) {
        throw 'Expected one minisign updater public key in tauri.conf.json.'
    }
    $decodedSignature = [Convert]::FromBase64String($signature)
    $temporarySignature = [IO.Path]::GetTempFileName()
    try {
        [IO.File]::WriteAllBytes($temporarySignature, $decodedSignature)
        & $minisign.Source -V -m $installer.FullName -x $temporarySignature -P $keyLines[0] -q
        if ($LASTEXITCODE -ne 0) {
            throw 'Updater signature does not verify against the final installer and configured public key.'
        }
        Write-Host 'Verified updater signature against the final installer.'
    }
    finally {
        Remove-Item -LiteralPath $temporarySignature -Force
    }
}

$assetName = "SayAll-Windows-$version-x64-setup.exe"
$signatureAssetName = "$assetName.sig"
$downloadUrl = "https://github.com/$Repository/releases/download/$Tag/$assetName"
$pubDate = ([DateTimeOffset]::UtcNow).ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")

$manifest = [ordered]@{
    version = $version
    notes = "无线麦 SayAll Windows 版 $Tag"
    pub_date = $pubDate
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            url = $downloadUrl
            signature = $signature
        }
    }
}

New-Item -ItemType Directory -Force -Path $stagingDirectory | Out-Null
$utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
$manifestPath = Join-Path $stagingDirectory "latest.json"
[System.IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 6), $utf8WithoutBom)

$stagedInstaller = Join-Path $stagingDirectory $assetName
$stagedSignature = Join-Path $stagingDirectory $signatureAssetName
Copy-Item -LiteralPath $installer.FullName -Destination $stagedInstaller -Force
Copy-Item -LiteralPath $signaturePath -Destination $stagedSignature -Force

$checksumLines = @(
    "$((Get-FileHash -Algorithm SHA256 -LiteralPath $stagedInstaller).Hash.ToLowerInvariant())  $assetName",
    "$((Get-FileHash -Algorithm SHA256 -LiteralPath $stagedSignature).Hash.ToLowerInvariant())  $signatureAssetName"
)
[System.IO.File]::WriteAllText((Join-Path $stagingDirectory "SHA256SUMS.txt"), ($checksumLines -join "`n") + "`n", $utf8WithoutBom)

# 自检：清单内容必须能被 updater 按静态 JSON 契约解析回来（version/url/signature）。
$parsed = Get-Content -Raw -Encoding UTF8 $manifestPath | ConvertFrom-Json
if ($parsed.version -ne $version -or $parsed.platforms.'windows-x86_64'.url -ne $downloadUrl -or $parsed.platforms.'windows-x86_64'.signature -ne $signature) {
    throw "latest.json round-trip self-check failed: $manifestPath"
}

Write-Host "Updater manifest generated: $manifestPath"
Write-Host "Version: $version (tag $Tag)"
Write-Host "Download URL: $downloadUrl"
Write-Host "Signature length: $($signature.Length) chars"
Write-Host "Staged release assets:"
foreach ($name in @("latest.json", $assetName, $signatureAssetName, "SHA256SUMS.txt")) {
    Write-Host "- $name"
}
