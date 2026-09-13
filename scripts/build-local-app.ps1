param([switch]$Offline)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & pnpm build
    if ($LASTEXITCODE) { throw 'Frontend build failed' }
    $buildArgs = @('build', '--locked', '--release', '-p', 'sayall-windows-app', '--features', 'custom-protocol')
    if ($Offline) { $buildArgs += '--offline' }
    $previousChannel = $env:SAYALL_BUILD_CHANNEL
    try {
        $env:SAYALL_BUILD_CHANNEL = 'local'
        & cargo @buildArgs
        if ($LASTEXITCODE) { throw 'Standalone application build failed' }
    } finally { $env:SAYALL_BUILD_CHANNEL = $previousChannel }
    Write-Output 'Built target/release/sayall-windows-app.exe with embedded frontend; no localhost server is needed.'
    Write-Output 'This command does not install, launch, sign, or publish the application.'
} finally { Pop-Location }
