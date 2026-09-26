/**
 * rc003_agent.js 的逻辑回归台（不需要 frida、不需要提权、不需要遥控器）。
 *
 * 为什么有这一层：`agent_selftest.py` 验证的是**传输与生命周期**（要 frida、要哑进程），
 * 它碰不到"这份报告到底动不动、改成什么样"的判定。而 2026-09-26 的真机事故恰好发生在
 * 这条判定上——哨兵键（canary，主页 0x004A）已下发、日志显示 `targets:applied ... canary=1`，
 * 用户按主页键光标照常跳到行首。根因：onEnter 的早退门禁用的是**上报**集合（三键）而不是
 * **清空**集合，于是只含哨兵键的报告在门禁处就被 return，下面那段用 clearUsages 的清空
 * 循环永远执行不到。哨兵键从第一天起就是死代码，而"按主页无反应"这条判据被静默地
 * 变成"恒不成立"——看起来像"拦截没生效"，实际是"拦截根本没被触发"。
 *
 * 做法：把 agent 脚本**原样**加载进 vm 上下文，补齐 frida 才有的全局（Socket / Process /
 * Interceptor / Memory 全部是可控桩），然后调用 `installHook()` 拿到它真正注册的那个
 * `onEnter`，用**假指针**喂一份 9 字节报告进去，断言缓冲区被改成什么样。
 * 不复刻任何一份逻辑——改回去就会红。
 *
 * 运行：node hardware/RC003/helper/agent/agent_logic_test.mjs [agent.js]
 *      第二参数指向另一份 agent 用来做**阳性对照**：把修复退回去，用例必须 FAIL。
 *      只断言"当前文件通过"而不验证"缺陷版本会红"，等于没验证判据有没有分辨力。
 * 退出码：0 全通过 / 1 有用例失败
 */

import { readFileSync } from 'node:fs';
import { createContext, runInContext } from 'node:vm';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const AGENT = process.argv[2] ? resolve(HERE, process.argv[2]) : join(HERE, 'rc003_agent.js');

const results = [];
function check(name, cond, detail = '') {
  results.push({ name, ok: !!cond, detail });
}

/* ---------------------------------------------------------------- 桩 */

/** 9 字节键盘报告：report_id(1) + modifiers(1) + reserved(1) + 3 × uint16 usage(LE)。 */
function report(...usages) {
  const b = new Uint8Array(9);
  b[0] = 0x01;
  for (let i = 0; i < usages.length && i < 3; i++) {
    const o = 3 + i * 2;
    b[o] = usages[i] & 0xff;
    b[o + 1] = (usages[i] >> 8) & 0xff;
  }
  return b;
}

function toArrayBuffer(u8) {
  const ab = new ArrayBuffer(u8.length);
  new Uint8Array(ab).set(u8);
  return ab;
}

/** 假指针：agent 只会用它做 readU8 / readByteArray / writeByteArray。 */
function fakePtr(initial) {
  const buf = Uint8Array.from(initial);
  return {
    buf,
    isNull: () => false,
    readU8: () => buf[0],
    readByteArray(n) {
      const ab = new ArrayBuffer(n);
      const v = new Uint8Array(ab);
      for (let i = 0; i < n && i < buf.length; i++) v[i] = buf[i];
      return ab;
    },
    writeByteArray(ab) {
      const v = new Uint8Array(ab);
      for (let i = 0; i < v.length && i < buf.length; i++) buf[i] = v[i];
    },
  };
}

const num = (n) => ({ toUInt32: () => n, isNull: () => false });

/** NtDeviceIoControlFile 的 10 个参数（只有 5..9 被 agent 用到）。 */
function fakeArgs(outPtr) {
  const a = new Array(10).fill(null).map(() => num(0));
  a[5] = num(0x80018483);                                   // IoControlCode
  a[6] = { isNull: () => false, readByteArray: () => toArrayBuffer(Uint8Array.from([0, 0, 0, 0, 0x02, 0x01, 0, 0])) };
  a[7] = num(8);                                            // InputBufferLength
  a[8] = outPtr;                                            // OutputBuffer
  a[9] = num(9);                                            // OutputBufferLength
  return a;
}

const handlers = {};
const pending = [];                                          // 永不 settle 的 read => 桩连接不会"断线"

const sandbox = {
  console,
  rpc: {},
  Process: {
    id: 4242,
    arch: 'x64',
    getModuleByName: () => ({ getExportByName: () => ({ isNull: () => false }) }),
  },
  Interceptor: {
    attach(_fn, cb) { handlers.onEnter = cb.onEnter; handlers.onLeave = cb.onLeave; },
  },
  Memory: { protect() { return true; } },
  Socket: {
    connect() {
      return Promise.resolve({
        setNoDelay() {},
        close() {},
        output: { write() {} },
        input: { read: () => new Promise((_res, _rej) => { pending.push(1); }) },
      });
    },
  },
  // init()/heartbeat() 不会在测试里调用；这两个桩只是防止它们被误触发后真的排上定时器。
  setInterval: () => 0,
  setTimeout: () => 0,
};

const src = readFileSync(AGENT, 'utf8');
const ctx = createContext(sandbox);
runInContext(src, ctx, { filename: 'rc003_agent.js' });

/* ------------------------------------------------- 连上桩助手（租约要靠它） */

ctx.params.port = 47831;
ctx.connectOnce().catch(() => {});
await new Promise((r) => setImmediate(r));                   // 让 connectOnce 的 then 落地
check('桩助手已连接（租约前置条件）', ctx.connected === true, `connected=${ctx.connected}`);
ctx.handleCommand('{"type":"renew"}');                       // 首次续约 = 武装时刻

/* ---------------------------------------------------------------- 挂 hook */

ctx.installHook();
check('installHook 注册了 onEnter', typeof handlers.onEnter === 'function');

function press(...usages) {
  const ptr = fakePtr(report(...usages));
  const self = {};
  handlers.onEnter.call(self, fakeArgs(ptr));
  return { ptr, self };
}

/* --------------------------------- 1. 默认（无哨兵键）：非目标键一字节不动 */

{
  const { ptr, self } = press(0x0028);                       // 确定键
  check('默认：确定键不被改写',
    ptr.buf[3] === 0x28 && self.hit === true && self.cleared === false,
    `buf3=0x${ptr.buf[3].toString(16)} hit=${self.hit} cleared=${self.cleared}`);
}

{
  const { ptr } = press(0x004a);                             // 主页键
  check('默认：主页键不被改写', ptr.buf[3] === 0x4a);
}

/* ------------------------------------------- 2. 三键：清空 + 计数 + 边沿 */

{
  const before = ctx.stat.target_hits;
  const { ptr, self } = press(0x00f1);
  check('三键：usage 槽被清零', ptr.buf[3] === 0 && ptr.buf[4] === 0,
    `buf=${Array.from(ptr.buf.slice(0, 9)).map((b) => b.toString(16)).join(',')}`);
  check('三键：target_hits +1', ctx.stat.target_hits === before + 1);
  check('三键：cleared=true（onLeave 才会回写）', self.cleared === true);
  check('三键：report_id / modifiers 未被动', ptr.buf[0] === 0x01);
}

/* ------------------------- 3. 下发哨兵键（助手 --canary-usage 0x4A 走这条） */

ctx.handleCommand(JSON.stringify({
  type: 'targets', report: [0x00f1, 0x0080, 0x0081], clear: [0x00f1, 0x0080, 0x0081, 0x004a],
}));
check('targets 被接受（targets_applied=1）',
  ctx.stat.targets_applied === 1, `applied=${ctx.stat.targets_applied} rejected=${ctx.stat.targets_rejected}`);

/* ------- 4. 本次缺陷的正主：只含哨兵键的报告必须真被清掉（不只是"门禁放行"） */

{
  const before = ctx.stat.canary_hits;
  const { ptr, self } = press(0x004a);
  check('哨兵键：usage 槽被清零（2026-09-26 缺陷）',
    ptr.buf[3] === 0 && ptr.buf[4] === 0,
    `buf3=0x${ptr.buf[3].toString(16)} buf4=0x${ptr.buf[4].toString(16)}`);
  check('哨兵键：cleared=true', self.cleared === true);
  check('哨兵键：计入 canary_hits', ctx.stat.canary_hits === before + 1,
    `canary_hits=${ctx.stat.canary_hits}`);
}

{
  const before = ctx.stat.target_hits;
  const { ptr } = press(0x004a);
  check('哨兵键：不计入 target_hits（不污染 edges 交叉校验）',
    ctx.stat.target_hits === before && ptr.buf[3] === 0);
}

/* --------------------------- 5. 启用哨兵后，非目标键仍然一字节不动 */

{
  const { ptr } = press(0x0028);
  check('启用哨兵后：确定键仍不被改写', ptr.buf[3] === 0x28);
}

/* --------------------------- 6. onLeave 回写（不留残留补丁） */

{
  const { ptr, self } = press(0x004a);
  handlers.onLeave.call(self, 0);
  check('onLeave 复原哨兵键报告', ptr.buf[3] === 0x4a && ptr.buf[4] === 0,
    `buf3=0x${ptr.buf[3].toString(16)}`);
}

/* --------------------------- 7. 护栏仍然生效 */

{
  const rejected = ctx.stat.targets_rejected;
  ctx.handleCommand(JSON.stringify({ type: 'targets', report: [0x004a], clear: [0x004a] }));
  check('篡改上报集合仍被拒', ctx.stat.targets_rejected === rejected + 1);
  ctx.handleCommand(JSON.stringify({ type: 'targets', report: [0x00f1, 0x0080, 0x0081], clear: [0x004a] }));
  check('清空集合不含三键仍被拒', ctx.stat.targets_rejected === rejected + 2);
}

/* --------------------------- 8. 静态：门禁必须挂在清空集合上 */

check('源码含 clearSetIn（清空集合视角）', src.includes('function clearSetIn('));
check('onEnter 门禁调用 shouldTouchReport',
  src.includes('if (!shouldTouchReport(bytes)) return;'));

const failed = results.filter((r) => !r.ok);
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'}  ${r.name}${r.detail ? '  -- ' + r.detail : ''}`);
}
console.log(`\n${results.length - failed.length}/${results.length} 通过`);
process.exit(failed.length === 0 ? 0 : 1);
