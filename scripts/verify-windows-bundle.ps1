$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $repositoryRoot "src-tauri/tauri.conf.json"
$compatibilitySourcePath = Join-Path $repositoryRoot "crates/sayall-windows/src/compatibility.rs"
$bundleDirectory = Join-Path $repositoryRoot "target/release/bundle/nsis"
$artifactDirectory = Join-Path $repositoryRoot "artifacts/windows-preview"

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
if ($installerHooks -notmatch '(?m)^!define SAYALL_MINIMUM_WINDOWS_BUILD 17763$') {
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
$compatibilitySource = Get-Content -Raw -Encoding UTF8 $compatibilitySourcePath
if ($compatibilitySource -notmatch 'WindowsVersion::new\(10, 0, 17_763\)') {
    throw "Runtime and NSIS minimum Windows versions must both remain Windows 10 build 17763"
}

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
