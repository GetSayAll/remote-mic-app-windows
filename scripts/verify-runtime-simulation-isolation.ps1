$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$productionExecutable = Join-Path $repositoryRoot "target/release/sayall-windows-app.exe"
$frontendDirectory = Join-Path $repositoryRoot "dist"
$forbiddenTokens = @(
    "windows-ci-simulation",
    "run_runtime_simulation_voice_session",
    "complete_runtime_simulation_smoke"
)

$files = @()
if (Test-Path -LiteralPath $productionExecutable -PathType Leaf) {
    $files += Get-Item -LiteralPath $productionExecutable
}
if (Test-Path -LiteralPath $frontendDirectory -PathType Container) {
    $files += Get-ChildItem -LiteralPath $frontendDirectory -File -Recurse
}
if ($files.Count -lt 2) {
    throw "Production runtime isolation check could not find the executable and frontend assets"
}

foreach ($file in $files) {
    $contents = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($file.FullName))
    foreach ($token in $forbiddenTokens) {
        if ($contents.Contains($token, [StringComparison]::Ordinal)) {
            throw "Production build contains runtime simulation token '$token' in $($file.FullName)"
        }
    }
}

Write-Host "Verified production build excludes Windows runtime simulation frontend and commands"
