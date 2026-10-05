// Isolated installer contracts. Does not run the product installer or touch settings.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8');
const code = (s) => s.split(/\r?\n/).map((l) => l.split(';')[0]).join('\n');

test('interactive reinstall offers both choices while unattended installation stays overlay', () => {
  const config = JSON.parse(read('src-tauri/tauri.conf.json'));
  assert.equal(config.bundle.windows.nsis.template, 'windows/installer.nsi');
  const source = code(read('src-tauri/windows/installer.nsi'));
  assert.match(source, /Page custom SayAllReinstallPage SayAllReinstallPageLeave/);
  assert.match(source, /安装前卸载/);
  assert.match(source, /请勿卸载/);
  assert.match(source, /Function SayAllReinstallPage[^]*?\$PassiveMode = 1[^]*?\$UpdateMode = 1[^]*?\$\{Silent\}/);
  assert.match(source, /RequestExecutionLevel highest/);
  assert.match(source, /ReadRegStr \$SayAllPreviousInstallDirectory[^]*?IfFileExists "\$SayAllPreviousInstallDirectory\\\$\{MAINBINARYNAME\}.exe" \+2 0\s+StrCpy \$SayAllPreviousInstallDirectory ""/);
  assert.match(source, /Function RestorePreviousInstallLocation/);
  assert.match(source, /Section Uninstall/);
  assert.match(source, /WriteUninstaller/);
  assert.equal(config.bundle.windows.allowDowngrades, false);
});

test('permission preflight precedes cleanup and current-package uninstall preserves update state', () => {
  const source = code(read('src-tauri/windows/installer.nsi'));
  const early = source.match(/Section EarlyChecks[^]*?SectionEnd/)[0];
  assert.match(early, /SayAllRequireWritableDirectory "\$INSTDIR" install/);
  const install = source.match(/Section Install[^]*?SectionEnd/)[0];
  assert.ok(install.indexOf('NSIS_HOOK_PREINSTALL') < install.indexOf('SayAllUninstallBeforeInstall'));
  assert.ok(install.indexOf('SayAllUninstallBeforeInstall') < install.indexOf('SetOutPath'));
  const hooks = code(read('src-tauri/windows/installer-hooks.nsh'));
  assert.match(hooks, /WriteUninstaller "\$PLUGINSDIR\\SayAllReinstallUninstall.exe"/);
  assert.match(hooks, /ExecWait[^\n]*SayAllReinstallUninstall.exe[^\n]*\/S \/UPDATE _\?=/);
  assert.match(hooks, /CreateFileW[^\n]*i 6, i 7[^\n]*i 3, i 0x02000000/);
  assert.match(hooks, /GetFileAttributesW[^\n]*\?e[^]*?Pop \$R7[^]*?\$R7 != 2\s+\$\{AndIf\} \$R7 != 3\s+Goto sayall_access_failed_[^]*?\$\{GetParent\}/);
  const uninstall = source.match(/Section Uninstall[^]*?SectionEnd/)[0];
  assert.match(uninstall, /\$UpdateMode = 1[^]*?SayAllRecycleProductFiles[^]*?\$\{Else\}[^]*?Delete/);
  assert.match(uninstall, /\$DeleteAppDataCheckboxState = 1\s+\$\{AndIf\} \$UpdateMode <> 1/);
  assert.match(uninstall, /\$UpdateMode <> 1\s+DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run"/);
});

test('every payload write fails closed before registry commit and retired-file cleanup', () => {
  const source = code(read('src-tauri/windows/installer.nsi'));
  const install = source.slice(source.indexOf('Section Install'), source.indexOf('SectionEnd', source.indexOf('Section Install')));
  assert.doesNotMatch(source, /!insertmacro CheckIfAppIsRunning|KillProcess|TerminateProcess/);
  assert.doesNotMatch(install, /Delete\s|RMDir\s|UninstallString.*Exec/);
  assert.match(install, /SayAllAssertAppStopped install/);
  assert.match(source, /SayAllAssertAppStopped uninstall/);
  for (const line of install.split('\n').filter((l) => /^\s*(File |WriteUninstaller )/.test(l))) {
    const after = install.slice(install.indexOf(line) + line.length);
    assert.match(after, /^\s*!insertmacro SayAllRequireWriteSuccess\s+\S+/);
  }
  const hooks = code(read('src-tauri/windows/installer-hooks.nsh'));
  assert.match(hooks, /SayAllRecycleRetiredFiles/);
  assert.match(hooks, /SayAllCleanupHelper\.exe|sayall-helper\.exe/);
  assert.match(hooks, /-CleanupHelperPath/);
  assert.doesNotMatch(hooks, /KillProcess|TerminateProcess|taskkill|Stop-Process/i);
});

test('one current-run installer result identifies fixed components without resource paths', () => {
  const source = code(read('src-tauri/windows/installer.nsi'));
  assert.equal((source.match(/Function \.onInstFailed\b/g) || []).length, 1);
  assert.equal((source.match(/Function \.onInstSuccess\b/g) || []).length, 1);
  assert.match(source, /SayAllLogInstallResult w start installer/);
  assert.match(source, /SayAllLogInstallResult a completed installer/);
  assert.match(source, /SayAllLogInstallResult a failed "\$SayAllInstallStage"/);
  const hooks = code(read('src-tauri/windows/installer-hooks.nsh'));
  const logger = hooks.match(/!macro SayAllLogInstallResult[^]*?!macroend/)[0];
  assert.doesNotMatch(logger, /SetErrorLevel|Exec|\$INSTDIR|\$LOCALAPPDATA/);
  assert.match(logger, /FileSeek \$R7 0 END/);
  for (const name of ['gadget', 'helper', 'license_attribution', 'license_frida']) {
    assert.match(hooks, new RegExp('StrCpy \\$SayAllInstallStage ' + name + '\\b'));
  }
});
