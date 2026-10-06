// Compile the production template against a tiny payload; never run the result.
// Scalar values come from an existing Tauri render, so this requires a prior build.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const generatedDir = process.env.SAYALL_NSIS_GENERATED_DIR || path.join(root, 'target/release/nsis/x64');
const generated = fs.readFileSync(path.join(generatedDir, 'installer.nsi'), 'utf8');
let source = fs.readFileSync(path.join(root, 'src-tauri/windows/installer.nsi'), 'utf8');
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const directory = path.join(root, 'target/dev/unified-input');
fs.mkdirSync(directory, { recursive: true });
const payload = path.join(directory, 'overlay-compile-payload.exe');
fs.writeFileSync(payload, 'Compile-only fixture, never execute.');
const values = {};
for (const match of source.matchAll(/!define (\w+) "\{\{(\w+)\}\}"/g)) {
  const actual = generated.match(new RegExp('!define ' + match[1] + ' "(.*)"'));
  if (!actual) throw new Error('Missing prior Tauri value: ' + match[1]);
  values[match[2]] = actual[1];
}
Object.assign(values, { compression: 'none', signed_plugins_path: '',
  installer_hooks: path.join(root, 'src-tauri/windows/installer-hooks.nsh'),
  main_binary_path: payload, out_file: path.join(directory, 'overlay-compile-only.exe'),
  uninstaller_sign_cmd: '' });
source = source.replace(/\{\{#if (\w+)\}\}([\s\S]*?)\{\{\/if\}\}/g,
  (_, key, body) => values[key] ? body : '');
source = source.replace(/\{\{#each file_associations[^}]*\}\}[\s\S]*?\{\{\/each\}\}\s*\{\{\/each\}\}/g, '');
const directories = [...new Set(config.bundle.resources.map((r) => path.dirname(r)).filter((d) => d !== '.'))];
const lists = { languages: config.bundle.windows.nsis.languages,
  language_files: config.bundle.windows.nsis.languages.map((l) => path.join(generatedDir, l + '.nsh')),
  resources_dirs: directories, resources_ancestors: directories,
  resources: config.bundle.resources.map((r) => [payload, r.replaceAll('/', '\\')]),
  binaries: [], deep_link_protocols: [] };
source = source.replace(/\{\{#each (\w+)[^}]*\}\}([\s\S]*?)\{\{\/each\}\}/g, (_, key, body) => {
  if (!(key in lists)) throw new Error('Unsupported fixture list: ' + key);
  return lists[key].map((value) => body.replaceAll('{{this.[1]}}', value[1] || '')
    .replaceAll('{{no-escape @key}}', payload).replaceAll('{{this}}', value)).join('');
});
source = source.replace(/\{\{(\w+)\}\}/g, (_, key) => {
  if (!(key in values)) throw new Error('Missing fixture value: ' + key);
  return values[key];
});
if (source.includes('{{')) throw new Error('Unrendered template block');
const fixture = path.join(directory, 'overlay-compile-only.nsi');
// Imported Tauri metadata/includes are read-only. Compilation runs in the owned
// fixture directory, and every generated file including OutFile stays here.
source = '!addincludedir "' + generatedDir.replaceAll('\\', '/') + '"\n' + source;
fs.writeFileSync(fixture, source);
// Unsigned Tauri bundles render the actual toolset's Plugins/x86-unicode/additional
// directory. Use that same toolset; do not depend on a developer's local cache.
const compiler = path.resolve(values.additional_plugins_path, '../../..', 'makensis.exe');
if (!fs.existsSync(compiler)) throw new Error('The preceding unsigned Tauri build did not provide its NSIS compiler');
const result = spawnSync(compiler, ['/NOCD', '/INPUTCHARSET', 'UTF8', '/V2', fixture], { cwd: directory, encoding: 'utf8' });
fs.writeFileSync(path.join(directory, 'installer-overlay-compile.log'), (result.stdout || '') + (result.stderr || ''));
if (result.error || result.status !== 0) throw new Error('NSIS compile failed; inspect installer-overlay-compile.log');
console.log('overlay-template: actual NSIS compilation passed; fixture was not executed');
