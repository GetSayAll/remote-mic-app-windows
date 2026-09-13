param(
    [Parameter(Mandatory=$true)][string]$BuildDirectory,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][string]$CertificateThumbprint,
    [Parameter(Mandatory=$true)][string]$Inf2Cat
)
$ErrorActionPreference = 'Stop'
# The signing certificate must be in CurrentUser\My and have a local private key.
# This script never exports the private key or imports a trusted root.
$CertificateThumbprint = $CertificateThumbprint.Replace(' ', '')
if ($CertificateThumbprint -notmatch '^[A-Fa-f0-9]{40}$') { throw 'Invalid certificate thumbprint' }
$cert = Get-Item -LiteralPath ("Cert:\CurrentUser\My\" + $CertificateThumbprint)
if (-not $cert.HasPrivateKey -or $cert.NotAfter -le (Get-Date)) { throw 'An unexpired signing certificate with a private key is required' }
$out = [IO.Path]::GetFullPath($OutputDirectory)
if ((Test-Path -LiteralPath $out) -and @(Get-ChildItem -LiteralPath $out -Force).Count) { throw 'Choose a new or empty output directory' }
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item -LiteralPath (Join-Path $BuildDirectory 'SayAllThreeButtonFilter.sys') -Destination $out
foreach ($file in @('SayAllThreeButtonFilter.inf','Install.ps1','Uninstall.ps1','Common.ps1','Status.ps1','README.md','LICENSE.RemoteMapper')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $out
}
Export-Certificate -Cert $cert -FilePath (Join-Path $out 'SayAllThreeButtonFilter.cer') | Out-Null
& signtool.exe sign /s My /sha1 $CertificateThumbprint /fd SHA256 (Join-Path $out 'SayAllThreeButtonFilter.sys')
if ($LASTEXITCODE) { throw 'SYS signing failed' }
# The catalog MUST be generated after signing the SYS because its bytes changed.
& $Inf2Cat "/driver:$out" /os:10_X64 /uselocaltime
if ($LASTEXITCODE) { throw 'Catalog generation failed' }
& signtool.exe sign /s My /sha1 $CertificateThumbprint /fd SHA256 (Join-Path $out 'SayAllThreeButtonFilter.cat')
if ($LASTEXITCODE) { throw 'CAT signing failed' }
$manifest = [ordered]@{}
Get-ChildItem -LiteralPath $out -File | Sort-Object Name | ForEach-Object {
    $manifest[$_.Name] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $out 'SHA256.json') -Encoding UTF8
Write-Output 'Test package created. It has not been trusted, installed, or hardware-verified.'
