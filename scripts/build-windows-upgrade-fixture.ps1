$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $repositoryRoot "src-tauri/tauri.conf.json"
$bundleDirectory = Join-Path $repositoryRoot "target/release/bundle/nsis"
$fixtureDirectory = Join-Path $repositoryRoot "target/upgrade-fixtures"
$overridePath = Join-Path $env:RUNNER_TEMP "sayall-predecessor-tauri-config.json"
$predecessorVersion = [Version]"0.0.1"
$config = Get-Content -Raw -Encoding UTF8 -LiteralPath $configPath | ConvertFrom-Json
$currentVersion = [Version]$config.version
if ($predecessorVersion -ge $currentVersion) {
    throw "Predecessor fixture version $predecessorVersion must remain lower than current $currentVersion"
}

@{ version = $predecessorVersion.ToString() } |
    ConvertTo-Json |
    Set-Content -Encoding UTF8 -LiteralPath $overridePath

Push-Location $repositoryRoot
try {
    & pnpm tauri build --bundles nsis --config $overridePath --ci --ignore-version-mismatches
    if ($LASTEXITCODE -ne 0) {
        throw "Building predecessor NSIS fixture failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$installers = @(Get-ChildItem -LiteralPath $bundleDirectory -Filter "*-setup.exe" -File)
if ($installers.Count -ne 1) {
    throw "Expected one predecessor NSIS installer, found $($installers.Count)"
}
$installer = $installers[0]
New-Item -ItemType Directory -Force -Path $fixtureDirectory | Out-Null
$fixturePath = Join-Path $fixtureDirectory "sayall-predecessor-$predecessorVersion-setup.exe"
Move-Item -LiteralPath $installer.FullName -Destination $fixturePath

$signature = Get-AuthenticodeSignature -FilePath $fixturePath
if ($signature.Status -ne "NotSigned") {
    throw "Predecessor CI fixture must be unsigned; observed $($signature.Status)"
}
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $fixturePath).Hash.ToLowerInvariant()
Write-Host "Built predecessor installer fixture: $fixturePath"
Write-Host "Predecessor version: $predecessorVersion"
Write-Host "Predecessor SHA-256: $hash"
