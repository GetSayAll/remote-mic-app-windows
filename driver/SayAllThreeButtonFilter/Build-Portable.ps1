param(
    [Parameter(Mandatory=$true)][string]$WdkRoot,
    [Parameter(Mandatory=$true)][string]$SdkRoot
)
$ErrorActionPreference = 'Stop'
# Run in an x64 MSVC environment. Roots point to the `c` directories of the
# Microsoft.Windows.WDK.x64 / Microsoft.Windows.SDK.cpp NuGet packages.
$out = Join-Path $PSScriptRoot 'build'
New-Item -ItemType Directory -Force $out | Out-Null
Push-Location $out
try {
    & cl.exe /nologo /W4 /WX /O2 /MT "$PSScriptRoot/remap.c" "$PSScriptRoot/remap_test.c" /Fe:remap_test.exe
    if ($LASTEXITCODE) { throw 'Report test compilation failed' }
    & ./remap_test.exe
    if ($LASTEXITCODE) { throw 'Report tests failed' }
    $includes = @(
        "/I$WdkRoot/Include/10.0.26100.0/km",
        "/I$WdkRoot/Include/10.0.26100.0/km/crt",
        "/I$WdkRoot/Include/wdf/kmdf/1.15",
        "/I$SdkRoot/Include/10.0.26100.0/shared"
    )
    & cl.exe /nologo /c /W4 /WX /O2 /GS /kernel /Zp8 /wd4324 /D_AMD64_ /DAMD64 /D_WIN64 /DWINVER=0x0A00 /D_WIN32_WINNT=0x0A00 /DNTDDI_VERSION=0x0A000000 @includes "$PSScriptRoot/driver.c" "$PSScriptRoot/remap.c"
    if ($LASTEXITCODE) { throw 'Driver compilation failed' }
    & link.exe /NOLOGO /DRIVER /SUBSYSTEM:NATIVE,10.00 /ENTRY:FxDriverEntry /NODEFAULTLIB /MACHINE:X64 /DYNAMICBASE /NXCOMPAT /INTEGRITYCHECK /OPT:REF /OPT:ICF /OUT:SayAllThreeButtonFilter.sys driver.obj remap.obj "/LIBPATH:$WdkRoot/Lib/10.0.26100.0/km/x64" "/LIBPATH:$WdkRoot/Lib/wdf/kmdf/x64/1.15" ntoskrnl.lib hal.lib BufferOverflowK.lib WdfLdr.lib WdfDriverEntry.lib
    if ($LASTEXITCODE) { throw 'Driver link failed' }
    Copy-Item -LiteralPath "$PSScriptRoot/SayAllThreeButtonFilter.inf" -Destination $out
    & "$WdkRoot/tools/10.0.26100.0/x64/InfVerif.exe" /w "$out/SayAllThreeButtonFilter.inf"
    if ($LASTEXITCODE) { throw 'INF verification failed' }
    & "$WdkRoot/bin/10.0.26100.0/x86/Inf2Cat.exe" "/driver:$out" /os:10_X64 /uselocaltime
    if ($LASTEXITCODE) { throw 'Catalog creation failed' }
    Write-Output 'PASS: driver compiled and catalog generated; package is not signed or installed.'
} finally {
    Pop-Location
}

