$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $repositoryRoot "src-tauri/tauri.conf.json"
$bundleDirectory = Join-Path $repositoryRoot "target/release/bundle/nsis"
$config = Get-Content -Raw -Encoding UTF8 $configPath | ConvertFrom-Json
$productName = $config.productName
$publisher = $config.bundle.publisher
$startMenuFolderName = $config.bundle.windows.nsis.startMenuFolder
$appConfigDirectory = Join-Path $env:APPDATA $config.identifier
$preservationMarker = Join-Path $appConfigDirectory "ci-uninstall-preservation-marker.txt"
$appProcess = $null
$normalUninstallCompleted = $false

function Get-PropertyValue($object, [string] $name) {
    $property = $object.PSObject.Properties[$name]
    if ($null -eq $property) {
        return $null
    }
    $property.Value
}

function Get-SayAllUninstallEntries {
    $uninstallRoots = @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    $entries = foreach ($root in $uninstallRoots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) {
            continue
        }
        foreach ($key in Get-ChildItem -LiteralPath $root) {
            $entry = Get-ItemProperty -LiteralPath $key.PSPath
            if (
                (Get-PropertyValue $entry "DisplayName") -eq $productName -and
                (Get-PropertyValue $entry "Publisher") -eq $publisher
            ) {
                $entry
            }
        }
    }
    @($entries)
}

function Split-ExecutableCommand([string] $command) {
    $trimmed = $command.Trim()
    if ($trimmed.StartsWith('"')) {
        $closingQuote = $trimmed.IndexOf('"', 1)
        if ($closingQuote -lt 2) {
            throw "Invalid quoted executable command: $command"
        }
        return [pscustomobject]@{
            FilePath = $trimmed.Substring(1, $closingQuote - 1)
            Arguments = $trimmed.Substring($closingQuote + 1).Trim()
        }
    }

    $match = [regex]::Match($trimmed, '^(?<path>.*?\.exe)(?:\s+(?<arguments>.*))?$', 'IgnoreCase')
    if (-not $match.Success) {
        throw "Uninstall command does not contain an executable: $command"
    }
    [pscustomobject]@{
        FilePath = $match.Groups['path'].Value
        Arguments = $match.Groups['arguments'].Value.Trim()
    }
}

function Invoke-SilentUninstall($entry) {
    $quietUninstallString = Get-PropertyValue $entry "QuietUninstallString"
    $uninstallString = Get-PropertyValue $entry "UninstallString"
    $command = if (-not [string]::IsNullOrWhiteSpace($quietUninstallString)) {
        $quietUninstallString
    } else {
        $uninstallString
    }
    if ([string]::IsNullOrWhiteSpace($command)) {
        throw "SayAll uninstall registry entry has no uninstall command"
    }

    $parts = Split-ExecutableCommand $command
    if (-not (Test-Path -LiteralPath $parts.FilePath -PathType Leaf)) {
        throw "SayAll uninstaller is missing: $($parts.FilePath)"
    }
    $arguments = $parts.Arguments
    if ($arguments -notmatch '(?i)(^|\s)/S($|\s)') {
        $arguments = "$arguments /S".Trim()
    }
    $process = Start-Process -FilePath $parts.FilePath -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Silent SayAll uninstall failed with exit code $($process.ExitCode)"
    }
}

$installers = @(Get-ChildItem -LiteralPath $bundleDirectory -Filter "*-setup.exe" -File)
if ($installers.Count -ne 1) {
    throw "Expected exactly one NSIS installer, found $($installers.Count)"
}
if (@(Get-SayAllUninstallEntries).Count -ne 0) {
    throw "A SayAll installation already exists before the CI lifecycle test"
}

try {
    $installerProcess = Start-Process -FilePath $installers[0].FullName -ArgumentList "/S" -Wait -PassThru
    if ($installerProcess.ExitCode -ne 0) {
        throw "Silent SayAll install failed with exit code $($installerProcess.ExitCode)"
    }

    $entries = @(Get-SayAllUninstallEntries)
    if ($entries.Count -ne 1) {
        throw "Expected one current-user SayAll uninstall entry after install, found $($entries.Count)"
    }
    $entry = $entries[0]
    $registeredInstallLocation = Get-PropertyValue $entry "InstallLocation"
    $installedVersion = Get-PropertyValue $entry "DisplayVersion"
    $mainBinaryName = Get-PropertyValue $entry "MainBinaryName"
    if ([string]::IsNullOrWhiteSpace($registeredInstallLocation)) {
        throw "SayAll uninstall entry has no InstallLocation"
    }
    if ($installedVersion -ne $config.version) {
        throw "Installed SayAll version $installedVersion does not match $($config.version)"
    }
    if (
        [string]::IsNullOrWhiteSpace($mainBinaryName) -or
        [IO.Path]::GetFileName($mainBinaryName) -ne $mainBinaryName -or
        [IO.Path]::GetExtension($mainBinaryName) -ne ".exe"
    ) {
        throw "SayAll uninstall entry has an invalid MainBinaryName: $mainBinaryName"
    }

    $installLocation = [IO.Path]::GetFullPath($registeredInstallLocation.Trim().Trim('"')).TrimEnd('\')
    $localAppDataRoot = [IO.Path]::GetFullPath($env:LOCALAPPDATA).TrimEnd('\')
    if (-not $installLocation.StartsWith("$localAppDataRoot\", [StringComparison]::OrdinalIgnoreCase)) {
        throw "Current-user installer wrote outside LOCALAPPDATA: $installLocation"
    }
    if (-not (Test-Path -LiteralPath $installLocation -PathType Container)) {
        throw "Installed SayAll directory is missing: $installLocation"
    }

    $startMenuRoot = [Environment]::GetFolderPath("StartMenu")
    $startMenuDirectory = Join-Path (Join-Path $startMenuRoot "Programs") $startMenuFolderName
    if (-not (Test-Path -LiteralPath $startMenuDirectory -PathType Container)) {
        throw "SayAll Start Menu folder is missing: $startMenuDirectory"
    }
    $shortcuts = @(Get-ChildItem -LiteralPath $startMenuDirectory -Filter "*.lnk" -File -Recurse)
    if ($shortcuts.Count -ne 1) {
        throw "Expected one SayAll Start Menu shortcut, found $($shortcuts.Count)"
    }
    if ($shortcuts[0].Name -ne "$productName.lnk") {
        throw "Unexpected SayAll Start Menu shortcut name: $($shortcuts[0].Name)"
    }
    $appExecutable = Join-Path $installLocation $mainBinaryName
    if (-not (Test-Path -LiteralPath $appExecutable -PathType Leaf)) {
        throw "Installed SayAll executable is missing: $appExecutable"
    }

    New-Item -ItemType Directory -Force -Path $appConfigDirectory | Out-Null
    $utf8WithoutBom = [Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText($preservationMarker, "CI uninstall preservation marker`n", $utf8WithoutBom)

    $appProcess = Start-Process -FilePath $appExecutable -PassThru
    Start-Sleep -Seconds 8
    $appProcess.Refresh()
    if ($appProcess.HasExited) {
        throw "Installed SayAll exited during the 8-second launch smoke test with code $($appProcess.ExitCode)"
    }
    Stop-Process -Id $appProcess.Id -Force
    $appProcess.WaitForExit()
    $appProcess = $null

    Invoke-SilentUninstall $entry
    $normalUninstallCompleted = $true

    if (@(Get-SayAllUninstallEntries).Count -ne 0) {
        throw "SayAll uninstall registry entry remains after silent uninstall"
    }
    if (Test-Path -LiteralPath $appExecutable) {
        throw "SayAll executable remains after silent uninstall: $appExecutable"
    }
    if (Test-Path -LiteralPath $installLocation) {
        throw "SayAll install directory remains after silent uninstall: $installLocation"
    }
    if (Test-Path -LiteralPath $startMenuDirectory) {
        throw "SayAll Start Menu folder remains after silent uninstall: $startMenuDirectory"
    }
    if (-not (Test-Path -LiteralPath $preservationMarker -PathType Leaf)) {
        throw "Silent uninstall unexpectedly removed the SayAll app-config marker"
    }

    Write-Host "Verified silent current-user install and uninstall: $($installers[0].Name)"
    Write-Host "Install location: $installLocation"
    Write-Host "Launch smoke test: process remained alive for 8 seconds"
    Write-Host "App config retained: $appConfigDirectory"
    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
        @"
### Windows NSIS lifecycle

- `/S` current-user install: passed
- HKCU uninstall entry and one Start Menu shortcut: passed
- installed process alive for 8 seconds: passed
- `/S` uninstall removed app files, shortcut and uninstall entry: passed
- `%APPDATA%\\$($config.identifier)` marker retained: passed

This does not validate visible UI rendering, SmartScreen, Windows 10 1809, real hardware, or a signed public installer.
"@ | Add-Content -Encoding UTF8 $env:GITHUB_STEP_SUMMARY
    }
} finally {
    if ($null -ne $appProcess -and -not $appProcess.HasExited) {
        Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if (-not $normalUninstallCompleted) {
        foreach ($remainingEntry in @(Get-SayAllUninstallEntries)) {
            try {
                Invoke-SilentUninstall $remainingEntry
            } catch {
                Write-Warning "Best-effort cleanup failed: $($_.Exception.Message)"
            }
        }
    }
}
