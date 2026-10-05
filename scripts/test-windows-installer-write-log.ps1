$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root 'target/dev/unified-input/write-log-fixture'
[IO.Directory]::CreateDirectory($fixture) | Out-Null
$generatedDirectory = if ($env:SAYALL_NSIS_GENERATED_DIR) { $env:SAYALL_NSIS_GENERATED_DIR } else { Join-Path $root 'target/release/nsis/x64' }
$rendered = [IO.File]::ReadAllText((Join-Path $generatedDirectory 'installer.nsi'))
$plugins = [regex]::Match($rendered, '(?m)^!define ADDITIONALPLUGINSPATH "([^"]+)"').Groups[1].Value
if (-not $plugins) { throw 'A preceding unsigned Tauri bundle is required' }
$compiler = [IO.Path]::GetFullPath((Join-Path $plugins '../../../makensis.exe'))
$source = Join-Path $fixture 'new-payload.txt'
$target = Join-Path $fixture 'payload.bin'
$script = Join-Path $fixture 'writer.nsi'
$writer = Join-Path $fixture 'writer.exe'
$logPath = Join-Path $fixture 'logs/installer-result.log'
[IO.File]::WriteAllText($source, 'new payload')
[IO.File]::WriteAllText($target, 'old payload')
$text = @'
Unicode true
SilentInstall silent
RequestExecutionLevel user
!include LogicLib.nsh
!include "__HOOKS__"
OutFile "__WRITER__"
Function .onInit
  StrCpy $SayAllInstallResultDirectory "$EXEDIR\logs"
  !insertmacro SayAllLogInstallResult w start installer
FunctionEnd
Function .onInstFailed
  !insertmacro SayAllLogInstallResult a failed "$SayAllInstallStage"
FunctionEnd
Function .onInstSuccess
  !insertmacro SayAllLogInstallResult a completed installer
FunctionEnd
Section
  SetOutPath "$EXEDIR"
  SetOverwrite on
  StrCpy $SayAllInstallStage app
  ClearErrors
  File /oname=payload.bin "__SOURCE__"
  !insertmacro SayAllRequireWriteSuccess app
SectionEnd
'@.Replace('__HOOKS__',(Join-Path $root 'src-tauri/windows/installer-hooks.nsh')).Replace('__WRITER__',$writer).Replace('__SOURCE__',$source)
[IO.File]::WriteAllText($script,$text,[Text.UTF8Encoding]::new($false))
& $compiler /INPUTCHARSET UTF8 /V1 $script
if ($LASTEXITCODE -ne 0) { throw 'Write log fixture compilation failed' }
$lock = [IO.File]::Open($target,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try {
    $child = Start-Process -FilePath $writer -PassThru -WindowStyle Hidden
    try {
        if (-not $child.WaitForExit(10000)) { throw 'Own fixture did not finish; no process was killed' }
        if ($child.ExitCode -eq 0) { throw 'Locked payload falsely reported success' }
        if (-not (Test-Path -LiteralPath $logPath)) { throw 'Locked payload failure did not produce a component result log' }
        $log = [IO.File]::ReadAllText($logPath)
        if ($log -notmatch 'event=start component=installer' -or $log -notmatch 'event=failed component=app' -or $log -match 'event=completed') { throw 'Locked payload component or terminal result incorrect' }
    } finally { $child.Dispose() }
} finally { $lock.Dispose() }
if ([IO.File]::ReadAllText($target) -ne 'old payload') { throw 'Locked fixture payload was changed' }
$child = Start-Process -FilePath $writer -PassThru -WindowStyle Hidden
try {
    if (-not $child.WaitForExit(10000) -or $child.ExitCode -ne 0) { throw 'Unlocked fixture failed to finish successfully' }
} finally { $child.Dispose() }
$log = [IO.File]::ReadAllText($logPath)
if ($log -notmatch 'event=start component=installer' -or $log -notmatch 'event=completed component=installer' -or $log -match 'event=failed') { throw 'Current successful run did not replace the old failure log' }
if ([IO.File]::ReadAllText($target) -ne 'new payload') { throw 'Successful payload side effect missing' }
if ($log -match '[A-Za-z]:\\|new payload|old payload') { throw 'Log leaked a path or file content' }
Write-Output 'installer-write-log: locked target names app and fails; unlocked target replaces payload and records completed'

# The logger is also run with an unwritable destination. These checks use native
# NSIS error flags and error levels, not a model of their behavior.
$flagsWriter = Join-Path $fixture 'flags.exe'
$flagsScript = Join-Path $fixture 'flags.nsi'
[IO.File]::WriteAllText((Join-Path $fixture 'not-a-directory'), 'fixture')
$flagsSection = @'
Section
  StrCpy $R6 preserved6
  StrCpy $R7 preserved7
  SetErrorLevel 37
  ClearErrors
  !insertmacro SayAllLogInstallResult a probe logger
  IfErrors 0 +3
    SetErrorLevel 701
    Abort
  GetErrorLevel $R8
  ${If} $R8 != 37
    SetErrorLevel 702
    Abort
  ${EndIf}
  StrCpy $SayAllInstallResultDirectory "$EXEDIR\not-a-directory"
  SetErrors
  !insertmacro SayAllLogInstallResult a probe logger
  IfErrors +3 0
    SetErrorLevel 703
    Abort
  ClearErrors
  !insertmacro SayAllLogInstallResult a probe logger
  IfErrors 0 +3
    SetErrorLevel 704
    Abort
  GetErrorLevel $R8
  ${If} $R8 != 37
  ${OrIf} $R6 != preserved6
  ${OrIf} $R7 != preserved7
    SetErrorLevel 705
    Abort
  ${EndIf}
  SetErrorLevel 0
SectionEnd
'@
$flagText = [regex]::Replace($text.Replace($writer,$flagsWriter), '(?s)Section\r?\n.*?SectionEnd', $flagsSection)
[IO.File]::WriteAllText($flagsScript,$flagText,[Text.UTF8Encoding]::new($false))
& $compiler /INPUTCHARSET UTF8 /V1 $flagsScript
if ($LASTEXITCODE -ne 0) { throw 'Flag preservation fixture compilation failed' }
$child = Start-Process -FilePath $flagsWriter -PassThru -WindowStyle Hidden
try {
    if (-not $child.WaitForExit(10000) -or $child.ExitCode -ne 0) { throw "Logger flag preservation failed: $($child.ExitCode)" }
} finally { $child.Dispose() }
Write-Output 'installer-write-log: successful and failed log writes preserve NSIS error flag, error level and registers'
