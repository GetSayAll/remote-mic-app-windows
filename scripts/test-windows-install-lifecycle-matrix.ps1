$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $repositoryRoot "src-tauri/tauri.conf.json"
$bundleDirectory = Join-Path $repositoryRoot "target/release/bundle/nsis"
$fixtureDirectory = Join-Path $repositoryRoot "target/upgrade-fixtures"
$config = Get-Content -Raw -Encoding UTF8 -LiteralPath $configPath | ConvertFrom-Json
$productName = $config.productName
$publisher = $config.bundle.publisher
$currentVersion = [Version]$config.version
$startMenuFolderName = $config.bundle.windows.nsis.startMenuFolder
$appConfigDirectory = Join-Path $env:APPDATA $config.identifier
$settingsPath = Join-Path $appConfigDirectory "settings.json"
$mappingsPath = Join-Path $appConfigDirectory "button-mappings.json"
$preservationMarker = Join-Path $appConfigDirectory "ci-upgrade-preservation-marker.txt"
$appProcess = $null
$normalUninstallCompleted = $false

function Get-PropertyValue($object, [string] $name) {
    $property = $object.PSObject.Properties[$name]
    if ($null -eq $property) { return $null }
    $property.Value
}

function Get-SayAllUninstallEntries {
    $entries = foreach ($root in @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        foreach ($key in Get-ChildItem -LiteralPath $root) {
            $entry = Get-ItemProperty -LiteralPath $key.PSPath
            if (
                (Get-PropertyValue $entry "DisplayName") -eq $productName -and
                (Get-PropertyValue $entry "Publisher") -eq $publisher
            ) { $entry }
        }
    }
    @($entries)
}

function Invoke-Installer([string] $path, [int] $timeoutSeconds = 45) {
    $process = Start-Process -FilePath $path -ArgumentList "/S" -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($timeoutSeconds)
    while (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
    }
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "Installer did not finish within $timeoutSeconds seconds: $path"
    }
    $process.ExitCode
}

function Split-ExecutableCommand([string] $command) {
    $trimmed = $command.Trim()
    if ($trimmed.StartsWith('"')) {
        $closingQuote = $trimmed.IndexOf('"', 1)
        if ($closingQuote -lt 2) { throw "Invalid quoted executable command: $command" }
        return [pscustomobject]@{
            FilePath = $trimmed.Substring(1, $closingQuote - 1)
            Arguments = $trimmed.Substring($closingQuote + 1).Trim()
        }
    }
    $match = [regex]::Match($trimmed, '^(?<path>.*?\.exe)(?:\s+(?<arguments>.*))?$', 'IgnoreCase')
    if (-not $match.Success) { throw "Command does not contain an executable: $command" }
    [pscustomobject]@{
        FilePath = $match.Groups['path'].Value
        Arguments = $match.Groups['arguments'].Value.Trim()
    }
}

function Invoke-SilentUninstall($entry) {
    $command = Get-PropertyValue $entry "QuietUninstallString"
    if ([string]::IsNullOrWhiteSpace($command)) {
        $command = Get-PropertyValue $entry "UninstallString"
    }
    if ([string]::IsNullOrWhiteSpace($command)) {
        throw "SayAll uninstall registry entry has no uninstall command"
    }
    $parts = Split-ExecutableCommand $command
    $arguments = $parts.Arguments
    if ($arguments -notmatch '(?i)(^|\s)/S($|\s)') { $arguments = "$arguments /S".Trim() }
    $process = Start-Process -FilePath $parts.FilePath -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Silent uninstall failed with exit code $($process.ExitCode)"
    }
}

function Get-SingleInstallation([Version] $expectedVersion) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    $entries = @()
    $entry = $null
    $actualVersion = $null
    do {
        $entries = @(Get-SayAllUninstallEntries)
        if ($entries.Count -eq 1) {
            $entry = $entries[0]
            $actualVersion = [Version](Get-PropertyValue $entry "DisplayVersion")
            if ($actualVersion -eq $expectedVersion) { break }
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($entries.Count -ne 1) {
        throw "Expected one SayAll uninstall entry, found $($entries.Count)"
    }
    if ($actualVersion -ne $expectedVersion) {
        throw "Installed version $actualVersion does not match expected $expectedVersion"
    }
    $installLocation = [IO.Path]::GetFullPath(
        (Get-PropertyValue $entry "InstallLocation").Trim().Trim('"')
    ).TrimEnd('\')
    $mainBinaryName = Get-PropertyValue $entry "MainBinaryName"
    $appExecutable = Join-Path $installLocation $mainBinaryName
    if (-not (Test-Path -LiteralPath $appExecutable -PathType Leaf)) {
        throw "Installed executable is missing: $appExecutable"
    }
    [pscustomobject]@{
        Entry = $entry
        InstallLocation = $installLocation
        AppExecutable = $appExecutable
    }
}

function Assert-SingleShortcut {
    $startMenuRoot = [Environment]::GetFolderPath("StartMenu")
    $directory = Join-Path (Join-Path $startMenuRoot "Programs") $startMenuFolderName
    $shortcuts = @()
    if (Test-Path -LiteralPath $directory -PathType Container) {
        $shortcuts = @(Get-ChildItem -LiteralPath $directory -Filter "*.lnk" -File -Recurse)
    }
    if ($shortcuts.Count -ne 1 -or $shortcuts[0].Name -ne "$productName.lnk") {
        throw "Expected exactly one SayAll Start Menu shortcut"
    }
    $directory
}

$currentInstallers = @(Get-ChildItem -LiteralPath $bundleDirectory -Filter "*-setup.exe" -File)
$predecessorInstallers = @(Get-ChildItem -LiteralPath $fixtureDirectory -Filter "sayall-predecessor-*-setup.exe" -File)
if ($currentInstallers.Count -ne 1) { throw "Expected one current installer, found $($currentInstallers.Count)" }
if ($predecessorInstallers.Count -ne 1) { throw "Expected one predecessor installer, found $($predecessorInstallers.Count)" }
$currentInstaller = $currentInstallers[0]
$predecessorInstaller = $predecessorInstallers[0]
if ($predecessorInstaller.BaseName -notmatch '^sayall-predecessor-(?<version>\d+\.\d+\.\d+)-setup$') {
    throw "Cannot read predecessor version from $($predecessorInstaller.Name)"
}
$predecessorVersion = [Version]$Matches.version
if ($predecessorVersion -ge $currentVersion) {
    throw "Predecessor version $predecessorVersion must be lower than current $currentVersion"
}
if (@(Get-SayAllUninstallEntries).Count -ne 0) {
    throw "A SayAll installation already exists before the lifecycle matrix"
}

try {
    $predecessorExitCode = Invoke-Installer $predecessorInstaller.FullName
    if ($predecessorExitCode -ne 0) {
        throw "Predecessor install failed with exit code $predecessorExitCode"
    }
    $predecessorInstallation = Get-SingleInstallation $predecessorVersion
    $initialInstallLocation = $predecessorInstallation.InstallLocation
    $startMenuDirectory = Assert-SingleShortcut

    New-Item -ItemType Directory -Force -Path $appConfigDirectory | Out-Null
    $settings = [ordered]@{
        schema_version = 2
        selected_remote_id = "ci-upgrade-remote"
        audio_endpoint_id = "ci-upgrade-endpoint"
        audio_endpoint_name = "CABLE Input (CI Upgrade Fixture)"
        gain_db = 6.0
        voice_trigger_mode = "hold"
        launch_at_login = $false
        open_window_at_launch = $true
        usage_statistics = @{ days = @{ "2026-09-02" = @{ button_presses = 7; voice_sessions = 3; voice_seconds = 12.5 } } }
    }
    $mappings = @{ actions = @{ ok = @{ type = "shortcut"; chord = @{ keys = @("left_control", "c") } } } }
    $utf8WithoutBom = [Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText($settingsPath, ($settings | ConvertTo-Json -Depth 8), $utf8WithoutBom)
    [IO.File]::WriteAllText($mappingsPath, ($mappings | ConvertTo-Json -Depth 8), $utf8WithoutBom)
    [IO.File]::WriteAllText($preservationMarker, "upgrade-matrix-preserve`n", $utf8WithoutBom)
    $settingsHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $settingsPath).Hash
    $mappingsHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $mappingsPath).Hash

    $upgradeExitCode = Invoke-Installer $currentInstaller.FullName
    if ($upgradeExitCode -ne 0) { throw "Current upgrade failed with exit code $upgradeExitCode" }
    Start-Sleep -Seconds 2
    $upgradeEntries = @(Get-SayAllUninstallEntries)
    Write-Host "After current installer: $($upgradeEntries.Count) matching uninstall entries"
    foreach ($upgradeEntry in $upgradeEntries) {
        Write-Host "- DisplayVersion=$((Get-PropertyValue $upgradeEntry 'DisplayVersion')); InstallLocation=$((Get-PropertyValue $upgradeEntry 'InstallLocation'))"
    }
    $upgradePasses = 1
    if ($upgradeEntries.Count -eq 1 -and [Version](Get-PropertyValue $upgradeEntries[0] "DisplayVersion") -ne $currentVersion) {
        # NSIS may finish the previous uninstaller just after the first installer returns.
        # A second idempotent current-version install is the bounded convergence step.
        Start-Sleep -Seconds 5
        $upgradeExitCode = Invoke-Installer $currentInstaller.FullName
        if ($upgradeExitCode -ne 0) { throw "Current upgrade convergence install failed with exit code $upgradeExitCode" }
        $upgradePasses = 2
    }
    $currentInstallation = Get-SingleInstallation $currentVersion
    if ($currentInstallation.InstallLocation -ne $initialInstallLocation) {
        throw "Upgrade changed install location from $initialInstallLocation to $($currentInstallation.InstallLocation)"
    }
    $null = Assert-SingleShortcut
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $settingsPath).Hash -ne $settingsHash) {
        throw "Upgrade changed the seeded settings file"
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $mappingsPath).Hash -ne $mappingsHash) {
        throw "Upgrade changed the seeded button mappings file"
    }

    $currentExecutableHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $currentInstallation.AppExecutable).Hash
    $appProcess = Start-Process -FilePath $currentInstallation.AppExecutable -PassThru
    Start-Sleep -Seconds 8
    $appProcess.Refresh()
    if ($appProcess.HasExited) {
        throw "Upgraded application exited during the 8-second smoke test with code $($appProcess.ExitCode)"
    }
    Stop-Process -Id $appProcess.Id -Force
    $appProcess.WaitForExit()
    $appProcess = $null

    $downgradeExitCode = Invoke-Installer $predecessorInstaller.FullName
    $afterDowngrade = Get-SingleInstallation $currentVersion
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $afterDowngrade.AppExecutable).Hash -ne $currentExecutableHash) {
        throw "Downgrade attempt replaced the current executable"
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $settingsPath).Hash -ne $settingsHash) {
        throw "Downgrade attempt changed the settings file"
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $mappingsPath).Hash -ne $mappingsHash) {
        throw "Downgrade attempt changed the button mappings file"
    }
    $null = Assert-SingleShortcut

    Invoke-SilentUninstall $afterDowngrade.Entry
    $normalUninstallCompleted = $true
    if (@(Get-SayAllUninstallEntries).Count -ne 0) { throw "Uninstall entry remains after matrix uninstall" }
    if (Test-Path -LiteralPath $afterDowngrade.InstallLocation) { throw "Install directory remains after matrix uninstall" }
    if (Test-Path -LiteralPath $startMenuDirectory) { throw "Start Menu directory remains after matrix uninstall" }
    foreach ($path in @($settingsPath, $mappingsPath, $preservationMarker)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Uninstall unexpectedly removed preserved user data: $path"
        }
    }

    Write-Host "Verified NSIS lifecycle matrix: $predecessorVersion -> $currentVersion -> downgrade rejected -> uninstall"
    Write-Host "Current-version convergence installer passes: $upgradePasses"
    Write-Host "Downgrade installer exit code: $downgradeExitCode; installed version remained $currentVersion"
    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
        @"
### Windows NSIS lifecycle matrix

- predecessor `$predecessorVersion` current-user install: passed
- upgrade to `$currentVersion` with one uninstall entry and one shortcut: passed
- current-version convergence installer passes: `$upgradePasses`
- exact settings, mapping and usage-statistics fixture preservation: passed
- upgraded process alive for 8 seconds: passed
- predecessor re-run did not replace `$currentVersion` (exit code `$downgradeExitCode`): passed
- final uninstall removed program identity and retained user data: passed

This does not validate visible installer UI, real historical binaries, SmartScreen, or Windows 10 1809 / Windows 11 desktop behavior.
"@ | Add-Content -Encoding UTF8 $env:GITHUB_STEP_SUMMARY
    }
} finally {
    if ($null -ne $appProcess -and -not $appProcess.HasExited) {
        Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if (-not $normalUninstallCompleted) {
        foreach ($entry in @(Get-SayAllUninstallEntries)) {
            try { Invoke-SilentUninstall $entry } catch { Write-Warning $_.Exception.Message }
        }
    }
}
