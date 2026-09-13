const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, 'gadget_adapter.js'), 'utf8');
const flush = async () => { for (let i = 0; i < 12; i++) await new Promise(resolve => setImmediate(resolve)); };

function fixture(commands, delayConnect = false) {
  const writes = [];
  let connects = 0, closes = 0, starts = 0, stops = 0, deadline, resolveConnect;
  const connection = {
    input: {async read() { return Buffer.from(commands.shift() || ''); }},
    output: {async writeAll(bytes) { writes.push(JSON.parse(Buffer.from(bytes).toString())); }},
    async close() { closes++; }
  };
  const context = vm.createContext({
    RUN_PARAMETERS: {port: 31001, token: 'a'.repeat(64), capture: {seconds: 15}},
    Process: {id: 23}, rpc: {exports: {
      start(config) { starts++; assert.equal(config.seconds, 15); },
      stop() { stops++; vm.runInContext('reportSink({kind:"stopped"})', context); }
    }},
    Socket: {connect(options) {
      connects++; assert.equal(options.host, '127.0.0.1'); assert.equal(options.port, 31001);
      return delayConnect ? new Promise(resolve => { resolveConnect = resolve; }) : Promise.resolve(connection);
    }},
    setTimeout(fn) { deadline = fn; return 1; }, clearTimeout() {}
  });
  vm.runInContext('let reportSink = () => {};\n' + source, context);
  return {context, writes, expire: () => deadline(), resolve: () => resolveConnect(connection),
    counts: () => ({connects, closes, starts, stops})};
}

(async () => {
  const normal = fixture(['{"kind":"accepted"}\n{"kind":"stop"}\n']);
  normal.context.rpc.exports.init(); await flush();
  assert.deepEqual(normal.counts(), {connects: 1, closes: 1, starts: 1, stops: 1});
  assert.equal(normal.writes[0].kind, 'hello'); assert.equal(normal.writes[0].pid, 23);
  assert.equal(normal.writes.at(-1).kind, 'stopped');
  const bad = fixture(['{"kind":"unknown"}\n']);
  bad.context.rpc.exports.init(); await flush();
  assert.equal(bad.counts().starts, 0); assert.equal(bad.counts().closes, 1);
  const timeout = fixture([], true);
  timeout.context.rpc.exports.init(); timeout.expire(); await flush();
  timeout.resolve(); await flush();
  assert.deepEqual(timeout.counts(), {connects: 1, closes: 1, starts: 0, stops: 1});
  const eof = fixture(['{"kind":"accepted"}\n']);
  eof.context.rpc.exports.init(); await flush();
  assert.deepEqual(eof.counts(), {connects: 1, closes: 1, starts: 1, stops: 1});
  await eof.context.rpc.exports.dispose();
  assert.equal(eof.counts().stops, 1);
  console.log(JSON.stringify({tests: 4, result: 'passed', physicalHardware: false}));
})().catch(error => { console.error(error); process.exitCode = 1; });
