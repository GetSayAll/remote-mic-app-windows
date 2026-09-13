const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, 'hid_tap.js'), 'utf8');

function fixture() {
  const events = [];
  let callbacks;
  let detachCount = 0;
  let deadline;
  let currentName = '\\device\\rc003-test';
  let reads = 0;
  const memory = () => ({
    readU16: () => currentName.length * 2,
    add: () => ({readPointer: () => ({isNull: () => false, readUtf16String: () => currentName})})
  });
  const context = vm.createContext({
    rpc: {exports: {}}, send: event => events.push(JSON.parse(JSON.stringify(event))),
    Process: {platform: 'windows', arch: 'x64', pointerSize: 8, enumerateModules: () => [],
      getModuleByName: () => ({getExportByName: name => name})},
    NativeFunction: function () { return () => 0; }, Memory: {alloc: memory},
    Interceptor: {attach(_target, hook) { callbacks = hook; return {detach() { detachCount++; }}; }},
    setTimeout(fn) { deadline = fn; return 1; }, clearTimeout() {},
    setInterval() { return 2; }, clearInterval() {}
  });
  vm.runInContext(source, context);
  function start(proxy = true, renewable = false) {
    return context.rpc.exports.start({device: '\\device\\rc003-test', seconds: 30, proxy_diagnostics: proxy, renewable});
  }
  function io({bytes = [1, 0, 0, 0xf1, 0, 0, 0, 0, 0], result = 0,
               ioctl = 0x80018483, length = 9, information = 9, ios = 0, handle = '1'} = {}) {
    const args = [];
    args[0] = {toString: () => handle};
    args[4] = {isNull: () => false, readU32: () => ios,
      add: () => ({readU64: () => ({toString: () => String(information)})})};
    args[5] = {toUInt32: () => ioctl};
    args[8] = {isNull: () => false, readByteArray() { reads++; return Uint8Array.from(bytes).buffer; }};
    args[9] = {toUInt32: () => length};
    const state = {};
    callbacks.onEnter.call(state, args);
    callbacks.onLeave.call(state, {toUInt32: () => result});
  }
  return {context, events, start, io, name(value) { currentName = value; },
    reads: () => reads, detachCount: () => detachCount, expire: () => deadline()};
}

let cases = 0;
function test(name, run) { run(); cases++; console.log('passed: ' + name); }
test('three little-endian usages and repeat suppression', () => {
  const f = fixture(); f.start();
  f.io({bytes: [1, 0, 0, 0xf1, 0, 0x80, 0, 0x81, 0]});
  f.io({bytes: [1, 0, 0, 0x81, 0, 0xf1, 0, 0x80, 0]});
  assert.deepEqual(f.events.filter(e => e.kind === 'state'), [
    {kind: 'state', stream: 1, scope: 'rc003', active: ['back', 'volume_down', 'volume_up']}
  ]);
});
test('pending failed short and unrelated I/O never reads output', () => {
  const f = fixture(); f.start();
  f.io({result: 0x103}); f.io({result: 0xc0000022}); f.io({information: 8});
  f.io({ios: 0x103}); f.io({length: 121}); f.io({ioctl: 123});
  f.name('\\device\\other'); f.io();
  assert.equal(f.reads(), 0);
});
test('observed UMDF success with unspecified completion length is decoded', () => {
  const f = fixture(); f.start();
  f.io({information: 0});
  assert.deepEqual(f.events.filter(e => e.kind === 'state')[0].active, ['back']);
  f.io({information: 0, bytes: [1, 0, 0, 0, 0, 0, 0, 0, 0]});
  assert.deepEqual(f.events.filter(e => e.kind === 'state')[1].active, []);
  f.io({information: 0, result: 0x103});
  f.io({information: 0, bytes: [2, 0, 0, 0xf1, 0, 0, 0, 0, 0]});
  assert.equal(f.events.filter(e => e.kind === 'state').length, 2);
});
test('handle reuse rechecks source rather than trusting numeric handle', () => {
  const f = fixture(); f.start(); f.io();
  f.name('\\device\\other'); f.io(); assert.equal(f.reads(), 1);
  f.name('\\device\\umdfctrldev-00000000-0000-0000-0000-000000000000'); f.io();
  const states = f.events.filter(e => e.kind === 'state');
  assert.equal(states[1].scope, 'proxy_unverified'); assert.equal(states[1].stream, 2);
});
test('proxy capture can be disabled for shared HID hosts', () => {
  const f = fixture(); f.start(false);
  f.name('\\device\\umdfctrldev-00000000-0000-0000-0000-000000000000'); f.io();
  assert.equal(f.reads(), 0);
});
test('invalid frames do not fabricate releases or leak normal keys', () => {
  const f = fixture(); f.start(); f.io();
  f.io({bytes: [2, 0, 0, 0, 0, 0, 0, 0, 0]});
  assert.equal(f.events.filter(e => e.kind === 'state').length, 1);
  f.io({bytes: [1, 0, 0, 4, 0, 5, 0, 6, 0]});
  assert.deepEqual(f.events.filter(e => e.kind === 'state')[1].active, []);
  assert.equal(JSON.stringify(f.events).includes('rc003-test'), false);
});
test('deadline detaches, no capture after stop, no restart', () => {
  const f = fixture(); f.start(); f.io(); f.expire();
  const before = f.reads(); f.io(); assert.equal(f.reads(), before);
  assert.equal(f.detachCount(), 1);
  assert.throws(() => f.start(), /already_started/);
  f.context.rpc.exports.stop(); assert.equal(f.detachCount(), 1);
});
test('stream state bounded to 16 handle identities', () => {
  const f = fixture(); f.start();
  for (let i = 0; i < 100; i++) f.io({handle: String(i)});
  assert.equal(f.reads(), 16);
});
test('existing Gadget and malformed config fail closed', () => {
  const f = fixture();
  assert.throws(() => f.context.rpc.exports.start({device: 'all', seconds: 30}), /allowlist/);
  assert.throws(() => f.context.rpc.exports.start({device: '\\device\\test', seconds: 999}), /duration/);
  f.context.Process.enumerateModules = () => [{name: 'AxonkeyRC003HidTap_deadbeef.dll'}];
  assert.throws(() => f.start(), /gadget/);
});
test('renewable bridge lease still expires and cannot resurrect a stopped hook', () => {
  const f = fixture(); f.start(true, true);
  f.context.rpc.exports.renewLease(); f.io(); f.expire();
  assert.equal(f.detachCount(), 1);
  assert.throws(() => f.context.rpc.exports.renewLease(), /lease_not_active/);
  const diagnostic = fixture(); diagnostic.start();
  assert.throws(() => diagnostic.context.rpc.exports.renewLease(), /lease_not_active/);
});
console.log(JSON.stringify({tests: cases, result: 'passed', physicalHardware: false}));
