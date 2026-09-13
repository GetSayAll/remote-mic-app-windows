#Requires -Version 7.0
# Tests use temporary files and an in-memory certificate. No trust store, BCD,
# PnP device, or installed driver is changed.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Common.ps1')
$out = Join-Path $PSScriptRoot ('build/script-tests/' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $out | Out-Null
$rsa = [Security.Cryptography.RSA]::Create(2048)
$request = [Security.Cryptography.X509Certificates.CertificateRequest]::new(
    'CN=SayAll package validator fixture', $rsa,
    [Security.Cryptography.HashAlgorithmName]::SHA256,
    [Security.Cryptography.RSASignaturePadding]::Pkcs1)
$cert = $request.CreateSelfSigned([DateTimeOffset]::Now.AddMinutes(-1), [DateTimeOffset]::Now.AddHours(1))
$script:fixtureSigner = $cert.Thumbprint
$script:fixtureStatus = 'Valid'
function Get-AuthenticodeSignature {
    param([string]$LiteralPath)
    [pscustomobject]@{Status=$script:fixtureStatus;SignerCertificate=[pscustomobject]@{Thumbprint=$script:fixtureSigner}}
}
$names = @('SayAllThreeButtonFilter.sys','SayAllThreeButtonFilter.inf','SayAllThreeButtonFilter.cat','Install.ps1','Uninstall.ps1','Common.ps1','Status.ps1')
foreach ($name in $names) { Set-Content -LiteralPath (Join-Path $out $name) -Value 'fixture' }
[IO.File]::WriteAllBytes((Join-Path $out 'SayAllThreeButtonFilter.cer'), $cert.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
$manifest = [ordered]@{}
Get-ChildItem -LiteralPath $out -File | ForEach-Object { $manifest[$_.Name]=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
function Save-FixtureManifest { $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $out 'SHA256.json') -Encoding UTF8 }
function Expect-Rejected([string]$label) {
    $rejected = $false
    try { Assert-SayAllPackage $out } catch { $rejected = $true }
    if (-not $rejected) { throw "Expected rejection: $label" }
    Write-Output "PASS: $label rejected"
}
try {
    Save-FixtureManifest
    Assert-SayAllPackage $out
    Write-Output 'PASS: complete fixture accepted'
    Set-Content -LiteralPath (Join-Path $out 'Install.ps1') -Value 'tampered'
    Expect-Rejected 'tampered script'
    Set-Content -LiteralPath (Join-Path $out 'Install.ps1') -Value 'fixture'
    $saved = $manifest['Status.ps1']; $manifest.Remove('Status.ps1'); Save-FixtureManifest
    Expect-Rejected 'missing required checksum'
    $manifest['Status.ps1']=$saved
    $manifest['../escape']='0'*64; Save-FixtureManifest
    Expect-Rejected 'path traversal'
    $manifest.Remove('../escape'); Save-FixtureManifest
    $script:fixtureStatus='NotTrusted'
    Expect-Rejected 'untrusted signature'
    $script:fixtureStatus='Valid'; $script:fixtureSigner='0'*40
    Expect-Rejected 'unexpected signer'
    $script:fixtureSigner=$cert.Thumbprint
    Assert-SayAllPackage $out
    $errors = $null; $tokens = $null
    foreach ($file in Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1') {
        [Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors) | Out-Null
        if ($errors.Count) { throw "Script syntax errors: $($file.Name)" }
    }
    Write-Output 'PASS: all driver PowerShell scripts parse'
} finally { $cert.Dispose(); $rsa.Dispose() }
