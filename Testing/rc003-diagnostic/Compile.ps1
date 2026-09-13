$ErrorActionPreference = 'Stop'
$framework = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319'
$metadata = Join-Path $env:WINDIR 'System32\WinMetadata'
$references = @('System.dll', 'System.Core.dll', 'System.Windows.Forms.dll', 'System.Drawing.dll', 'System.Web.Extensions.dll')
$references += Join-Path $framework 'System.Runtime.InteropServices.WindowsRuntime.dll'
foreach ($facade in @('System.Runtime', 'System.Runtime.InteropServices', 'System.ObjectModel', 'System.Threading.Tasks')) {
    $references += Get-ChildItem -LiteralPath (Join-Path $env:WINDIR ('Microsoft.NET\assembly\GAC_MSIL\' + $facade)) -Recurse -Filter ($facade + '.dll') | Select-Object -First 1 -ExpandProperty FullName
}
$references += Get-ChildItem -LiteralPath $metadata -Filter '*.winmd' | ForEach-Object FullName
$arguments = @('/nologo', '/target:winexe', '/platform:x64', '/optimize+', ('/out:' + (Join-Path $PSScriptRoot 'RC003-Diagnostic.exe')))
$arguments += $references | ForEach-Object { '/reference:' + $_ }
$arguments += Join-Path $PSScriptRoot 'Diagnostic.cs'
& (Join-Path $framework 'csc.exe') @arguments
if ($LASTEXITCODE -ne 0) { throw 'Diagnostic build failed' }
