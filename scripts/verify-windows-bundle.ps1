$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $repositoryRoot "src-tauri/tauri.conf.json"
$compatibilitySourcePath = Join-Path $repositoryRoot "crates/sayall-windows/src/compatibility.rs"
$bundleDirectory = Join-Path $repositoryRoot "target/release/bundle/nsis"
$artifactDirectory = Join-Path $repositoryRoot "artifacts/windows-preview"
$applicationPath = Join-Path $repositoryRoot "target/release/sayall-windows-app.exe"

$config = Get-Content -Raw -Encoding UTF8 $configPath | ConvertFrom-Json
if ($config.productName -ne "无线麦 SayAll") {
    throw "Unexpected productName: $($config.productName)"
}
if ($config.identifier -ne "app.getsayall.remote-mic.windows") {
    throw "Unexpected application identifier: $($config.identifier)"
}
if ($config.bundle.publisher -ne "GetSayAll") {
    throw "Unexpected publisher: $($config.bundle.publisher)"
}
if ($config.bundle.windows.nsis.installMode -ne "currentUser") {
    throw "NSIS installer must remain a current-user installation"
}
if ($config.bundle.windows.allowDowngrades -ne $false) {
    throw "Windows installer must reject downgrades"
}
$expectedInstallerHooks = "windows/installer-hooks.nsh"
if ($config.bundle.windows.nsis.installerHooks -ne $expectedInstallerHooks) {
    throw "NSIS installer must reference $expectedInstallerHooks"
}
$installerHooksPath = Join-Path (Split-Path -Parent $configPath) $expectedInstallerHooks
if (-not (Test-Path -LiteralPath $installerHooksPath -PathType Leaf)) {
    throw "NSIS installer hooks file is missing: $installerHooksPath"
}
$installerHooks = Get-Content -Raw -Encoding UTF8 $installerHooksPath
if ($installerHooks -notmatch '(?m)^!define SAYALL_MINIMUM_WINDOWS_BUILD 17763\r?$') {
    throw "NSIS installer minimum Windows build must remain 17763"
}
if (
    $installerHooks -notmatch 'NSIS_HOOK_PREINSTALL' -or
    $installerHooks -notmatch 'AtLeastBuild' -or
    $installerHooks -notmatch 'SetErrorLevel 1633' -or
    $installerHooks -notmatch '(?m)^\s*Quit\s*$'
) {
    throw "NSIS installer must reject unsupported Windows versions before copying app files"
}
if (
    $installerHooks -notmatch '(?m)^!define SAYALL_DOWNGRADE_ERROR_LEVEL 1638\r?$' -or
    $installerHooks -notmatch 'ReadRegStr \$R8 SHCTX "\$\{UNINSTKEY\}" "DisplayVersion"' -or
    $installerHooks -notmatch 'nsis_tauri_utils::SemverCompare "\$\{VERSION\}" \$R8' -or
    $installerHooks -notmatch 'SetErrorLevel \$\{SAYALL_DOWNGRADE_ERROR_LEVEL\}'
) {
    throw "NSIS preinstall hook must reject silent downgrades before copying app files"
}
if (
    $installerHooks -notmatch 'NSIS_HOOK_POSTINSTALL' -or
    $installerHooks -notmatch 'SYSTEM\\CurrentControlSet\\Services\\VBAudioVACMME' -or
    $installerHooks -notmatch 'ReadRegStr \$R8 HKLM' -or
    $installerHooks -notmatch '\$\{IfNot\} \$\{Silent\}' -or
    $installerHooks -notmatch 'https://vb-audio\.com/Cable/' -or
    $installerHooks -notmatch 'ExecShell "open"'
) {
    throw "NSIS postinstall hook must detect VB-CABLE and offer its official page only in interactive installs"
}
$compatibilitySource = Get-Content -Raw -Encoding UTF8 $compatibilitySourcePath
if ($compatibilitySource -notmatch 'WindowsVersion::new\(10, 0, 17_763\)') {
    throw "Runtime and NSIS minimum Windows versions must both remain Windows 10 build 17763"
}

# ── 载荷集合一致性（2026-10-07 issue206，arm64 双载荷）──────────────────
# 判据：出包时**暂存到 src-tauri/ 的载荷集合**就是安装包实际携带的集合——资源条目
# 写的是裸文件名，落盘路径即安装根目录（见 stage-bundle-resources.cjs 顶部）。
# arm64 两件是可选的：本机/部分环境没有 VS ARM64 工具集时暂存不到，不得因此误报
# 失败；但只要暂存到了，构建就必须用 arm64 覆盖配置声明它们，否则安装包会静默地
# 只带 x64——那正是 issue206 要消除的失败模式。
$srcTauriDir = Split-Path -Parent $configPath
$payloadNames = @(
    "sayall-helper.exe",
    "frida-gadget.dll",
    "sayall-helper-arm64.exe",
    "frida-gadget-arm64.dll"
)
$stagedPayloads = @($payloadNames | Where-Object { Test-Path -LiteralPath (Join-Path $srcTauriDir $_) -PathType Leaf })
if ($stagedPayloads -notcontains "sayall-helper.exe" -or $stagedPayloads -notcontains "frida-gadget.dll") {
    throw "x64 bundle resources were not staged in src-tauri: $srcTauriDir"
}
$arm64PayloadNames = @("sayall-helper-arm64.exe", "frida-gadget-arm64.dll")
$stagedArm64 = @($arm64PayloadNames | Where-Object { $stagedPayloads -contains $_ })
if ($stagedArm64.Count -ne 0 -and $stagedArm64.Count -ne $arm64PayloadNames.Count) {
    throw "Incomplete arm64 payload staging: $($stagedArm64 -join ', ')"
}
$arm64PayloadConfigPath = Join-Path $srcTauriDir "tauri.arm64-payload.conf.json"
if ($stagedArm64.Count -eq $arm64PayloadNames.Count) {
    if (-not (Test-Path -LiteralPath $arm64PayloadConfigPath -PathType Leaf)) {
        throw "arm64 payloads are staged but tauri.arm64-payload.conf.json is missing: the installer would silently omit them"
    }
    $arm64PayloadConfig = Get-Content -Raw -Encoding UTF8 $arm64PayloadConfigPath | ConvertFrom-Json
    $declaredPayloads = @($arm64PayloadConfig.bundle.resources)
    foreach ($name in $payloadNames) {
        if ($declaredPayloads -notcontains $name) {
            throw "arm64 payload override must declare $name"
        }
    }
    if ($declaredPayloads.Count -ne $payloadNames.Count) {
        throw "arm64 payload override must declare exactly $($payloadNames.Count) payloads, observed $($declaredPayloads.Count)"
    }
}
if ($env:SAYALL_REQUIRE_ARM64_PAYLOAD -eq "1" -and $stagedArm64.Count -ne $arm64PayloadNames.Count) {
    throw "SAYALL_REQUIRE_ARM64_PAYLOAD=1 but arm64 payloads were not staged: the published installer must carry them"
}
Write-Host "Verified bundle payload set ($($stagedPayloads.Count)): $($stagedPayloads -join ', ')"

if (-not (Test-Path -LiteralPath $applicationPath -PathType Leaf)) {
    throw "Release application executable is missing: $applicationPath"
}
$applicationBytes = [System.IO.File]::ReadAllBytes($applicationPath)
if ($applicationBytes.Length -lt 0x100 -or $applicationBytes[0] -ne 0x4D -or $applicationBytes[1] -ne 0x5A) {
    throw "Release application is not a valid PE executable: $applicationPath"
}
$peOffset = [BitConverter]::ToInt32($applicationBytes, 0x3C)
if ($peOffset -lt 0 -or $peOffset + 0x5C -gt $applicationBytes.Length) {
    throw "Release application has an invalid PE header: $applicationPath"
}
if ($applicationBytes[$peOffset] -ne 0x50 -or $applicationBytes[$peOffset + 1] -ne 0x45 -or $applicationBytes[$peOffset + 2] -ne 0 -or $applicationBytes[$peOffset + 3] -ne 0) {
    throw "Release application is missing the PE signature: $applicationPath"
}
$optionalHeaderOffset = $peOffset + 24
$subsystem = [BitConverter]::ToUInt16($applicationBytes, $optionalHeaderOffset + 68)
if ($subsystem -ne 2) {
    throw "Release application must use the Windows GUI subsystem (2), observed $subsystem"
}
Write-Host "Verified Windows GUI subsystem for $([System.IO.Path]::GetFileName($applicationPath))"

$installers = @(Get-ChildItem -Path $bundleDirectory -Filter "*-setup.exe" -File)
if ($installers.Count -ne 1) {
    throw "Expected exactly one NSIS installer, found $($installers.Count)"
}
$installer = $installers[0]
if ($installer.Length -lt 1MB) {
    throw "NSIS installer is unexpectedly small: $($installer.Length) bytes"
}

$signature = Get-AuthenticodeSignature -FilePath $installer.FullName
if ($signature.Status -ne "NotSigned") {
    throw "CI preview must be explicitly unsigned; observed signature status $($signature.Status)"
}

New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$copiedInstaller = Join-Path $artifactDirectory $installer.Name
Copy-Item -LiteralPath $installer.FullName -Destination $copiedInstaller -Force
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $copiedInstaller).Hash.ToLowerInvariant()
$checksumPath = Join-Path $artifactDirectory "SHA256SUMS.txt"
$utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText($checksumPath, "$hash  $($installer.Name)`n", $utf8WithoutBom)

$metadata = [ordered]@{
    productName = $config.productName
    version = $config.version
    identifier = $config.identifier
    publisher = $config.bundle.publisher
    installer = $installer.Name
    sha256 = $hash
    signatureStatus = $signature.Status.ToString()
    sourceCommit = $env:GITHUB_SHA
    distributionStatus = "unsigned-ci-preview-not-for-public-release"
}
$metadata | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $artifactDirectory "build-metadata.json")

Write-Host "Verified unsigned NSIS preview: $($installer.Name)"
Write-Host "SHA-256: $hash"
