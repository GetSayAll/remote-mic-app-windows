param([ValidateSet('overlay', 'reinstall', 'refused', 'invalid-path', 'child-failed', 'new-parent')][string[]]$Scenarios = @('overlay', 'reinstall', 'refused', 'invalid-path', 'child-failed', 'new-parent'))
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root 'target/dev/unified-input/reinstall-flow-fixture'
[IO.Directory]::CreateDirectory($fixture) | Out-Null
$generatedDirectory = if ($env:SAYALL_NSIS_GENERATED_DIR) { $env:SAYALL_NSIS_GENERATED_DIR } else { Join-Path $root 'target/release/nsis/x64' }
$rendered = [IO.File]::ReadAllText((Join-Path $generatedDirectory 'installer.nsi'))
$plugins = [regex]::Match($rendered, '(?m)^!define ADDITIONALPLUGINSPATH "([^"]+)"').Groups[1].Value
if (-not $plugins) { throw 'A preceding unsigned Tauri bundle is required' }
$compiler = [IO.Path]::GetFullPath((Join-Path $plugins '../../../makensis.exe'))
$script = Join-Path $fixture 'flow.nsi'
$installer = Join-Path $fixture 'flow.exe'
$payload = Join-Path $fixture 'payload.txt'
$runId = [string][DateTime]::UtcNow.Ticks
$newDirectory = 'nested-' + $runId + '\new-install'
[IO.File]::WriteAllText($payload, 'new payload')
$text = @'
Unicode true
SilentInstall silent
RequestExecutionLevel user
!include LogicLib.nsh
!include FileFunc.nsh
!include "__HOOKS__"
!define MAINBINARYNAME "sayall-windows-app"
OutFile "__INSTALLER__"
Var Scenario
Var TestDirectory
Function .onInit
  ${GetOptions} $CMDLINE "/SCENARIO=" $Scenario
  StrCpy $INSTDIR "$EXEDIR\$Scenario"
  StrCpy $SayAllInstallResultDirectory "$INSTDIR\logs"
  StrCpy $SayAllPreviousInstallDirectory "$INSTDIR"
  StrCpy $SayAllReinstall 1
  ${If} $Scenario == overlay
    StrCpy $SayAllReinstall 0
  ${EndIf}
  ${If} $Scenario == refused
    StrCpy $INSTDIR "$INSTDIR\not-a-directory"
  ${EndIf}
  ${If} $Scenario == new-parent
    StrCpy $INSTDIR "$INSTDIR\__NEW_DIRECTORY__"
    StrCpy $SayAllReinstall 0
  ${EndIf}
  !insertmacro SayAllLogInstallResult w start installer
FunctionEnd
Function .onInstFailed
  !insertmacro SayAllLogInstallResult a failed "$SayAllInstallStage"
FunctionEnd
Function .onInstSuccess
  !insertmacro SayAllLogInstallResult a completed installer
FunctionEnd
Section
  StrCpy $TestDirectory "$INSTDIR"
  ${If} $Scenario == invalid-path
    ; NSIS sanitizes its built-in $INSTDIR. An ordinary variable preserves the
    ; invalid component so the production macro really observes error 123.
    StrCpy $TestDirectory "$INSTDIR\bad|directory"
  ${EndIf}
  !insertmacro SayAllRequireWritableDirectory "$TestDirectory" fixture
  ; This marker proves rejected preflight never reached the cleanup boundary.
  FileOpen $0 "$SayAllPreviousInstallDirectory\cleanup-entered-__RUN_ID__" w
  FileClose $0
  !insertmacro SayAllUninstallBeforeInstall
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File /oname=sayall-windows-app.exe "__PAYLOAD__"
  !insertmacro SayAllRequireWriteSuccess app
SectionEnd
Function un.onInit
  StrCpy $SayAllInstallResultDirectory "$INSTDIR\logs"
FunctionEnd
Section Uninstall
  ; Controlled child failure tests the real parent macro's fail-closed branch.
  IfFileExists "$INSTDIR\fail-uninstall" 0 +3
    SetErrorLevel 42
    Abort
  !insertmacro SayAllRecycleProductFiles
SectionEnd
'@.Replace('__HOOKS__', (Join-Path $root 'src-tauri/windows/installer-hooks.nsh')).Replace('__INSTALLER__', $installer).Replace('__PAYLOAD__', $payload).Replace('__NEW_DIRECTORY__', $newDirectory).Replace('__RUN_ID__', $runId)
[IO.File]::WriteAllText($script, $text, [Text.UTF8Encoding]::new($false))
& $compiler /INPUTCHARSET UTF8 /V1 $script
if ($LASTEXITCODE -ne 0) { throw 'Reinstall flow fixture compilation failed' }
foreach ($scenario in $Scenarios) {
    $case = Join-Path $fixture $scenario
    [IO.Directory]::CreateDirectory($case) | Out-Null
    [IO.File]::WriteAllText((Join-Path $case 'sayall-windows-app.exe'), 'old payload')
    [IO.File]::WriteAllText((Join-Path $case 'settings.json'), 'preserved settings')
    [IO.File]::WriteAllText((Join-Path $case 'capture-authorization.json'), 'preserved authorization')
    if ($scenario -eq 'refused') { [IO.File]::WriteAllText((Join-Path $case 'not-a-directory'), 'blocked') }
    if ($scenario -eq 'child-failed') { [IO.File]::WriteAllText((Join-Path $case 'fail-uninstall'), 'blocked') }
    # Recycle success is tested only on our text fixture, never a real app/install.
    $child = Start-Process -FilePath $installer -ArgumentList @('/S', "/SCENARIO=$scenario") -PassThru -WindowStyle Hidden
    try {
        if (-not $child.WaitForExit(20000)) { throw 'Own fixture did not finish; no process was killed' }
        $failed = $scenario -in @('refused', 'invalid-path', 'child-failed')
        if (($child.ExitCode -ne 0) -ne $failed) { throw "Unexpected fixture result for $scenario : $($child.ExitCode)" }
    } finally { $child.Dispose() }
    $log = [IO.File]::ReadAllText((Join-Path $case 'logs/installer-result.log'))
    $payloadDirectory = if ($scenario -eq 'new-parent') { Join-Path $case $newDirectory } else { $case }
    $actual = [IO.File]::ReadAllText((Join-Path $payloadDirectory 'sayall-windows-app.exe'))
    if ($failed) {
        if ($actual -ne 'old payload' -or $log -match 'event=completed component=installer') { throw 'Failed operation continued to install' }
    } elseif ($actual -ne 'new payload' -or $log -notmatch 'event=completed component=installer') { throw 'Automatic install did not complete' }
    if ($scenario -in @('refused', 'invalid-path') -and ((Test-Path -LiteralPath (Join-Path $case ('cleanup-entered-' + $runId))) -or $log -notmatch 'failed component=directory_access')) { throw 'Permission preflight did not fail before cleanup' }
    if ($scenario -eq 'child-failed' -and $log -notmatch 'failed component=reinstall_uninstall') { throw 'Child failure was not diagnosed' }
    if ($scenario -eq 'overlay' -and $log -match 'component=reinstall_uninstall|component=product_cleanup') { throw 'Overlay invoked uninstaller' }
    if ($scenario -eq 'reinstall' -and $log -notmatch 'event=completed component=reinstall_uninstall') { throw 'Uninstall completion not observed' }
    if ([IO.File]::ReadAllText((Join-Path $case 'settings.json')) -ne 'preserved settings' -or [IO.File]::ReadAllText((Join-Path $case 'capture-authorization.json')) -ne 'preserved authorization') { throw 'User state changed' }
    Write-Output "installer-reinstall-flow: $scenario passed"
}
