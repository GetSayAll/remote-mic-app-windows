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

/** 假句柄：agent 只用 toString() 作为来源键；没有 NativeFunction 时不查询对象名。 */
const fakeHandle = (name) => ({
  toString: () => name,
  isNull: () => false,
  toUInt32: () => 0,
});

/** NtDeviceIoControlFile 的 10 个参数（0 与 5..9 被 agent 用到）。 */
function fakeArgs(outPtr, source = 'h0') {
  const a = new Array(10).fill(null).map(() => num(0));
  a[0] = fakeHandle(source);                                // FileHandle = 报告来源
  a[5] = num(0x80018483);                                   // IoControlCode
  a[6] = { isNull: () => false, readByteArray: () => toArrayBuffer(Uint8Array.from([0, 0, 0, 0, 0x02, 0x01, 0, 0])) };
  a[7] = num(8);                                            // InputBufferLength
  a[8] = outPtr;                                            // OutputBuffer
  a[9] = num(9);                                            // OutputBufferLength
  return a;
}

const handlers = {};
const pending = [];                                          // 永不 settle 的 read => 桩连接不会"断线"
const sleeps = [];                                           // Thread.sleep 桩的调用记录（门内延迟判据）

/* 第二个 attach（NtClose）按导出名分流；Node 里没有 NativeFunction，
   NtQueryObject 返回 null，agent 走句柄字符串来源键。 */
const exports = {
  NtDeviceIoControlFile: { isNull: () => false, kind: 'ioctl' },
  NtClose: { isNull: () => false, kind: 'close' },
  NtQueryObject: null,
};

const sandbox = {
  console,
  rpc: {},
  Process: {
    id: 4242,
    arch: 'x64',
    pointerSize: 8,
    getModuleByName: () => ({ getExportByName: (name) => exports[name] || { isNull: () => false } }),
  },
  Interceptor: {
    attach(fn, cb) {
      if (fn && fn.kind === 'close') handlers.onClose = cb.onEnter;
      else { handlers.onEnter = cb.onEnter; handlers.onLeave = cb.onLeave; }
    },
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
  // Frida Thread.sleep 接收秒；只记录调用，不真的睡。
  Thread: {
    sleep(seconds) { sleeps.push(seconds); },
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

function pressOn(source, ...usages) {
  const ptr = fakePtr(report(...usages));
  const self = {};
  handlers.onEnter.call(self, fakeArgs(ptr, source));
  return { ptr, self };
}

function press(...usages) {
  return pressOn('h0', ...usages);
}

function closeHandle(source) {
  /* 旧 agent（阳性对照）没有 NtClose hook：保持 no-op，让来源相关用例红，
     而不是在测试台里抛异常。 */
  if (typeof handlers.onClose === 'function') handlers.onClose.call({}, [fakeHandle(source)]);
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

ctx.handleCommand(JSON.stringify({
  type: 'targets', generation: 1,
  report: [0x00f1, 0x0080, 0x0081], clear: [0x00f1, 0x0080, 0x0081],
}));

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
  type: 'targets', generation: 2,
  report: [0x00f1, 0x0080, 0x0081], clear: [0x00f1, 0x0080, 0x0081, 0x004a],
}));
check('targets 热更新被接受（targets_applied=2）',
  ctx.stat.targets_applied === 2, `applied=${ctx.stat.targets_applied} rejected=${ctx.stat.targets_rejected}`);

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
  const all = [0x00f1, 0x0028, 0x0035, 0x004a, 0x004f, 0x0050, 0x0051,
    0x0052, 0x0065, 0x0066, 0x007f, 0x0080, 0x0081];
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 3, report: all, clear: all }));
  for (const usage of all) {
    const { ptr } = press(usage);
    check(`动态全键：0x${usage.toString(16)} 被清零`, ptr.buf[3] === 0 && ptr.buf[4] === 0);
  }
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 4, report: [], clear: [] }));
  const { ptr } = press(0x004a);
  check('动态目标清空后恢复原始报告', ptr.buf[3] === 0x4a);
}

/* ------- 7.5 按设备来源隔离（2026-11-12 共享宿主现场） -------
   同一 WUDFHost 里另一台蓝牙键盘/散热器的空报告曾被当成遥控器的 UP，
   把长按提前结束。这里钉住：只有当前按下来源能结束自己的按下状态。 */

{
  ctx.handleCommand(JSON.stringify({
    type: 'targets', generation: 6, report: [0x00f1], clear: [0x00f1],
  }));
  ctx.activeSource = null;
  ctx.pressed = {};

  const beforeForeign = ctx.stat.foreign_reports;
  const beforeEdges = ctx.stat.edges_sent;
  const a = pressOn('device-a', 0x00f1);
  check('来源隔离：设备 A 按下 0x00f1 被清零', a.ptr.buf[3] === 0 && a.ptr.buf[4] === 0);
  check('来源隔离：设备 A 成为活动来源', ctx.activeSource === 'device-a');
  check('来源隔离：设备 A 的 DOWN 已上报', ctx.stat.edges_sent === beforeEdges + 1);

  const bEmpty = pressOn('device-b');
  check('来源隔离：设备 B 的空报告不清空设备 A 的按下状态',
    ctx.currentUsages().length === 1 && ctx.currentUsages()[0] === 0x00f1,
    `usages=${JSON.stringify(ctx.currentUsages())}`);
  check('来源隔离：设备 B 的空报告不产生释放边沿',
    ctx.stat.edges_sent === beforeEdges + 1,
    `edges=${ctx.stat.edges_sent} before=${beforeEdges}`);
  check('来源隔离：异源报告计数 +1', ctx.stat.foreign_reports === beforeForeign + 1,
    `foreign=${ctx.stat.foreign_reports} before=${beforeForeign}`);
  check('来源隔离：设备 B 的空报告未被改写', bEmpty.ptr.buf[3] === 0 && bEmpty.self.hit !== true);

  const bPress = pressOn('device-b', 0x00f1);
  check('来源隔离：设备 B 按下同一 usage 不被清空/不冒充 A',
    bPress.ptr.buf[3] === 0xf1 && bPress.ptr.buf[4] === 0 && ctx.activeSource === 'device-a');
  check('来源隔离：设备 B 的按下不产生边沿',
    ctx.stat.edges_sent === beforeEdges + 1 && bPress.self.hit !== true);

  const aUp = pressOn('device-a');
  check('来源隔离：设备 A 自己的空报告才产生 UP',
    ctx.currentUsages().length === 0 && ctx.activeSource === null
      && ctx.stat.edges_sent === beforeEdges + 2);
  check('来源隔离：A 的释放帧不被当作目标键改写', aUp.ptr.buf[3] === 0);

  const bTakeover = pressOn('device-b', 0x00f1);
  check('来源隔离：A 释放后 B 可作为新的活动来源', ctx.activeSource === 'device-b'
    && bTakeover.ptr.buf[3] === 0 && bTakeover.ptr.buf[4] === 0);

  const beforeClose = ctx.stat.forced_releases;
  closeHandle('device-b');
  check('来源隔离：活动来源句柄关闭时补发 UP 并清空归属',
    ctx.activeSource === null && ctx.currentUsages().length === 0
      && ctx.stat.forced_releases === beforeClose + 1,
    `active=${ctx.activeSource} forced=${ctx.stat.forced_releases} before=${beforeClose}`);

  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 7, report: [], clear: [] }));
}

/* --------------------------- 8. 护栏仍然生效 */

{
  const rejected = ctx.stat.targets_rejected;
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 5, report: [0x1234], clear: [0x1234] }));
  check('白名单外 usage 仍被拒', ctx.stat.targets_rejected === rejected + 1);
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 5, report: [0x00f1, 0x0080, 0x0081], clear: [0x004a] }));
  check('清空集合不含三键仍被拒', ctx.stat.targets_rejected === rejected + 2);
}

/* ----------- 9. 语音键热键合成（豆包支持，2026-09-28 行为验证通过后集成） ----------- */
/* 机制：语音键 0x003E 槽内替换成合成 usage（0x00E6=RightAlt）——电平跟随物理报告，
   物理释放即合成释放，无粘键路径。语音键 ATVV 会话走 BLE 协议层，不受影响；
   0x003E 在 Windows 输入流本来就未映射，替换零损失。 */

{
  const rejected = ctx.stat.synth_rejected || 0;
  ctx.handleCommand(JSON.stringify({ type: 'synth', from: 0x003E, to: 0x00E6 }));
  check('synth：合法配置被接受', (ctx.stat.synth_applied || 0) === 1,
    `applied=${ctx.stat.synth_applied} rejected=${ctx.stat.synth_rejected}`);

  const { ptr, self } = press(0x003E);
  check('synth：语音键槽被替换成 0x00E6',
    ptr.buf[3] === 0xe6 && ptr.buf[4] === 0,
    `buf3=0x${ptr.buf[3].toString(16)} buf4=0x${ptr.buf[4].toString(16)}`);
  check('synth：report_id / modifiers 未被动', ptr.buf[0] === 0x01 && ptr.buf[1] === 0);
  check('synth：不产生按键边沿（语音键不参与映射系统）', self.hit === true);
}

{
  const { ptr } = press(0x003E, 0x0028);
  check('synth：与其它键共存时只替换语音键槽',
    ptr.buf[3] === 0xe6 && ptr.buf[5] === 0x28 && ptr.buf[6] === 0,
    `buf=${Array.from(ptr.buf).map((b) => b.toString(16)).join(',')}`);
}

{
  const { ptr } = press();
  check('synth：物理释放帧全零——合成键随物理电平消失，无粘键路径',
    ptr.buf[3] === 0 && ptr.buf[4] === 0 && ptr.buf[5] === 0,
    `buf3=0x${ptr.buf[3].toString(16)}`);
}

{
  const rejected = ctx.stat.synth_rejected;
  ctx.handleCommand(JSON.stringify({ type: 'synth', from: 0x004A, to: 0x00E6 }));
  check('synth：from 非语音键被拒', ctx.stat.synth_rejected === rejected + 1);
  ctx.handleCommand(JSON.stringify({ type: 'synth', from: 0x003E, to: 0x1234 }));
  check('synth：to 不在合成白名单被拒', ctx.stat.synth_rejected === rejected + 2);
}

{
  ctx.handleCommand(JSON.stringify({ type: 'mode', clear: false }));
  const { ptr } = press(0x003E);
  check('synth：observe 模式不替换', ptr.buf[3] === 0x3e,
    `buf3=0x${ptr.buf[3].toString(16)}`);
  ctx.handleCommand(JSON.stringify({ type: 'mode' }));
}

{
  /* 租约过期（disarm/bye 同路径）：替换停止 → 报告回归语音键原样 → 0xE6 从
     报告中消失。HID 报告是状态语义，usage 消失本身就是 UP——无需补帧。 */
  ctx.handleCommand(JSON.stringify({ type: 'renew' }));
  press(0x003E);                                            // 替换激活（OS 收到 0xE6 DOWN）
  ctx.lastRenewAt = 0;                                      // 模拟租约过期
  const a = press(0x003E);
  check('synth：租约过期后停止替换，报告回归原样（0xE6 消失即 OS 收到 UP）',
    a.ptr.buf[3] === 0x3e,
    `buf3=0x${a.ptr.buf[3].toString(16)}`);
}

{
  ctx.handleCommand(JSON.stringify({ type: 'renew' }));
  ctx.handleCommand(JSON.stringify({ type: 'synth', off: true }));
  const { ptr } = press(0x003E);
  check('synth：off 后语音键恢复原样', ptr.buf[3] === 0x3e,
    `buf3=0x${ptr.buf[3].toString(16)}`);
}

/* --------- 10. 门内延迟：第一按 vs 输入法切换的赛跑（2026-10-03） --------- */
/* 应用在按下之后才切输入法（实测 ~53ms），而替换在按下帧通过时立即生效——先于
   切换完成，目标输入法收不到按下沿。应用声明能力（W 1 → gate 命令）后，按下帧
   改写前先睡 GATE_DELAY_MS，把呈现推到切换完成之后。
   阳性对照：对未含门内延迟的旧 agent 跑本组，「恰好睡一次」必然失败。 */

/* 上一节最后把 synth 关了——本节先恢复合成配置，否则替换路径根本不参与。 */
ctx.handleCommand(JSON.stringify({ type: 'synth', from: 0x003E, to: 0x00E6 }));

{
  const before = sleeps.length;
  const { ptr } = press(0x003E);
  check('gate 默认关：不延迟，替换照常',
    sleeps.length === before && ptr.buf[3] === 0xe6,
    `sleeps=${sleeps.length - before} buf3=0x${ptr.buf[3].toString(16)}`);
}

{
  ctx.handleCommand(JSON.stringify({ type: 'gate', on: true, delay_ms: 150 }));
  const before = sleeps.length;
  const beforeDelays = ctx.stat.gate_delays;
  const { ptr } = press(0x003E);
  check('gate 开：150ms 转为 0.15s，改写前恰好睡一次',
    sleeps.length === before + 1 && sleeps[sleeps.length - 1] === 0.15,
    `sleep_seconds=${JSON.stringify(sleeps.slice(before))}`);
  check('gate 开：替换仍然生效（0x00E6）',
    ptr.buf[3] === 0xe6, `buf3=0x${ptr.buf[3].toString(16)}`);
  check('gate 开：计数 gate_delays +1', ctx.stat.gate_delays === beforeDelays + 1);
}

for (const { name, command, seconds } of [
  { name: '省略 delay_ms 使用默认 150ms', command: {}, seconds: 0.15 },
  { name: 'delay_ms 为 0 仍使用默认 150ms', command: { delay_ms: 0 }, seconds: 0.15 },
  { name: '上限 2000ms 转为 2s', command: { delay_ms: 2000 }, seconds: 2 },
]) {
  ctx.handleCommand(JSON.stringify({ type: 'renew' }));
  ctx.handleCommand(JSON.stringify({ type: 'gate', on: true, ...command }));
  const before = sleeps.length;
  const { ptr } = press(0x003E);
  check(`gate 开：${name}，替换仍然生效`,
    sleeps.length === before + 1 && sleeps[sleeps.length - 1] === seconds && ptr.buf[3] === 0xe6,
    `sleep_seconds=${JSON.stringify(sleeps.slice(before))} buf3=0x${ptr.buf[3].toString(16)}`);
}

{
  const before = sleeps.length;
  const { ptr } = press();
  check('释放帧先保持 200ms 再回零（尾音排空）',
    sleeps.length === before + 1 && sleeps[sleeps.length - 1] === 0.2 && ptr.buf[3] === 0,
    `sleep_seconds=${JSON.stringify(sleeps.slice(before))} buf3=0x${ptr.buf[3].toString(16)}`);
  const before2 = sleeps.length;
  const { ptr: ptr2 } = press();
  check('释放保持只做一次：没有新的语音键按下时不重复延迟',
    sleeps.length === before2 && ptr2.buf[3] === 0,
    `sleeps=${sleeps.length - before2}`);
}

{
  ctx.handleCommand(JSON.stringify({ type: 'mode', clear: false }));
  const before = sleeps.length;
  const { ptr } = press(0x003E);
  check('gate 开但 observe 模式：不延迟也不替换',
    sleeps.length === before && ptr.buf[3] === 0x3e,
    `sleeps=${sleeps.length - before} buf3=0x${ptr.buf[3].toString(16)}`);
  ctx.handleCommand(JSON.stringify({ type: 'mode' }));
}

{
  ctx.handleCommand(JSON.stringify({ type: 'gate', on: false }));
  const before = sleeps.length;
  const { ptr } = press(0x003E);
  check('gate 关：恢复不延迟，替换照常',
    sleeps.length === before && ptr.buf[3] === 0xe6,
    `sleeps=${sleeps.length - before}`);
}

check('源码含 synthSetIn（合成视角进门禁，防 canary 死代码同款坑）',
  src.includes('function synthSetIn('));

/* --------------------------- 10. 静态：门禁必须挂在清空集合上 */

check('源码含 clearSetIn（清空集合视角）', src.includes('function clearSetIn('));
check('onEnter 门禁调用 shouldTouchReport',
  src.includes('if (!shouldTouchReport(bytes)) return;'));

/* --------- 11. 连接健康：租约过期必须走快速重连，不能误判成鉴权失败等 15s --------- */
/* 2026-10-21 现场：连接已经收到过 renew，但租约过期时 lastRenewAt 被清零；旧判据
   把这种普通停顿当成 auth_mismatch，于是一次抖动要等 4s + 15s。这里钉住分离判据：
   - 本次连接**从未**收到 renew → auth_mismatch（渐进退避，首次 2s）；
   - 本次连接**收到过** renew、之后静默 → rx_silence（1s）。
   阳性对照：把 `!everRenewed` 去回 `lastRenewAt === 0`，第一条会红。 */

async function freshStubConnection() {
  ctx.connected = false;
  ctx.sock = null;
  ctx.connecting = false;
  ctx.retryNotBefore = 0;
  await ctx.connectOnce();
}

await freshStubConnection();
check('新连接：everRenewed 初始为 false', ctx.everRenewed === false);
ctx.handleCommand('{"type":"renew"}');
check('收到续约：everRenewed=true', ctx.everRenewed === true);

/* 场景 A：收到过续约，随后租约过期/下行静默。 */
ctx.tConnect = Date.now() - 60000;
ctx.lastRenewAt = 0;                    // heartbeat 的租约过期分支会这样清
ctx.tLastRx = Date.now() - 4000;
ctx.stat.auth_rejected = 0;
ctx.stat.rx_timeouts = 0;
ctx.retryNotBefore = 0;
ctx.heartbeat();
check('租约过期但收到过续约：走 rx_silence，不判 auth_mismatch',
  ctx.stat.rx_timeouts === 1 && ctx.stat.auth_rejected === 0,
  `rx=${ctx.stat.rx_timeouts} auth=${ctx.stat.auth_rejected}`);
const fastDelay = ctx.retryNotBefore - Date.now();
check('rx_silence 退避约 1s（不再固定 15s）',
  fastDelay > 0 && fastDelay <= 1200, `delay_ms=${fastDelay}`);

/* 场景 B：连上了但本次连接从未收到续约（令牌不符/换代）。 */
await freshStubConnection();
ctx.tConnect = Date.now() - 5000;
ctx.lastRenewAt = 0;
ctx.tLastRx = Date.now();
ctx.stat.auth_rejected = 0;
ctx.stat.rx_timeouts = 0;
ctx.retryNotBefore = 0;
ctx.heartbeat();
check('从未收到续约：仍判 auth_mismatch',
  ctx.stat.auth_rejected === 1 && ctx.stat.rx_timeouts === 0,
  `rx=${ctx.stat.rx_timeouts} auth=${ctx.stat.auth_rejected}`);
const slowDelay = ctx.retryNotBefore - Date.now();
check('首次鉴权失败退避约 2s（渐进退避起点）',
  slowDelay > 0 && slowDelay <= 2200, `delay_ms=${slowDelay}`);
check('鉴权退避按失败次数渐进：2/4/8/15s 封顶',
  ctx.authBackoffMs(1) === 2000 && ctx.authBackoffMs(2) === 4000
    && ctx.authBackoffMs(3) === 8000 && ctx.authBackoffMs(9) === 15000,
  `seq=${[1, 2, 3, 9].map((n) => ctx.authBackoffMs(n)).join(',')}`);
check('新连接重置 everRenewed（不让上一代续约影响鉴权判定）',
  ctx.everRenewed === false);

/* -------- 12. 来源键：NtQueryObject 对象名归并 / 查询失败回退（2026-11-12） -------- */
/* 真实 Frida 里由 NativeFunction + Memory.alloc 完成；这里用最小桩验证
   `sourceKeyOf` 的 UNICODE_STRING 读取与“同对象名=同来源”的归并逻辑。
   旧 agent（阳性对照）没有 `sourceKeyOf`：记录一条失败并跳过本组，不能让
   阳性对照在测试台里崩掉——那会让“红”失去分辨力。 */
if (typeof ctx.sourceKeyOf === 'function') {
  const fakeNameBuffer = (name) => ({
    readU16: () => name.length * 2,
    add: () => ({
      readPointer: () => ({ isNull: () => false, readUtf16String: () => name }),
    }),
  });
  const originalAlloc = ctx.Memory.alloc;
  ctx.Memory.alloc = () => fakeNameBuffer('\\Device\\000000db');
  ctx.ntQueryObject = () => 0;
  const a = ctx.sourceKeyOf(fakeHandle('object-name-a'));
  const b = ctx.sourceKeyOf(fakeHandle('object-name-b'));
  check('来源键：设备对象名把不同句柄归并为同一来源',
    a === '\\Device\\000000db' && b === a, `a=${a} b=${b}`);

  /* 同一设备对象有两个句柄（不同 TLC）：关一个不能提前释放，关最后一个才释放。 */
  ctx.handleCommand(JSON.stringify({
    type: 'targets', generation: 8, report: [0x00f1], clear: [0x00f1],
  }));
  ctx.activeSource = null;
  ctx.pressed = {};
  delete ctx.sourceKeys['object-name-a'];
  delete ctx.sourceKeys['object-name-b'];
  ctx.sourceKeyOf(fakeHandle('tlc-a'));
  ctx.sourceKeyOf(fakeHandle('tlc-b'));
  pressOn('tlc-a', 0x00f1);
  const beforeOneClose = ctx.stat.forced_releases;
  closeHandle('tlc-a');
  check('来源隔离：同一设备多句柄关一个不提前 UP',
    ctx.activeSource === '\\Device\\000000db'
      && ctx.currentUsages().length === 1
      && ctx.stat.forced_releases === beforeOneClose,
    `active=${ctx.activeSource} forced=${ctx.stat.forced_releases}`);
  closeHandle('tlc-b');
  check('来源隔离：同一设备最后一个句柄关闭才补发 UP',
    ctx.activeSource === null && ctx.currentUsages().length === 0
      && ctx.stat.forced_releases === beforeOneClose + 1,
    `active=${ctx.activeSource} forced=${ctx.stat.forced_releases}`);
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 9, report: [], clear: [] }));

  ctx.ntQueryObject = null;
  ctx.Memory.alloc = originalAlloc;
  const c = ctx.sourceKeyOf(fakeHandle('fallback-handle'));
  check('来源键：NtQueryObject 不可用时回退句柄字符串',
    c === 'fallback-handle', `c=${c}`);
} else {
  check('来源键：agent 提供 sourceKeyOf', false, '旧 agent 无来源键');
}

/* -------- 13. 语音键释放后不能把 activeSource 永久钉住（2026-11-12 评审） -------- */
{
  await freshStubConnection();
  ctx.handleCommand('{"type":"renew"}');
  ctx.handleCommand('{"type":"mode","clear":true}');
  ctx.handleCommand('{"type":"gate","on":false}');
  ctx.handleCommand(JSON.stringify({ type: 'synth', from: 0x003e, to: 0x00e6 }));
  ctx.handleCommand(JSON.stringify({
    type: 'targets', generation: 10, report: [0x00f1], clear: [0x00f1],
  }));
  ctx.activeSource = null;
  ctx.pressed = {};

  const voice = pressOn('voice-source', 0x003e);
  check('来源归属：语音键按下成为活动来源',
    ctx.activeSource === 'voice-source' && voice.ptr.buf[3] === 0xe6);
  pressOn('voice-source');
  check('来源归属：语音键释放后清空来源，不永久占用', ctx.activeSource === null);

  pressOn('voice-source', 0x003e);
  check('来源归属：语音键按下时 synthHeld=true', ctx.synthHeld === true);
  ctx.releaseEdges('test');
  check('来源归属：强制释放清空合成键状态', ctx.synthHeld === false && ctx.activeSource === null);

  const foreignAfterVoice = pressOn('other-source', 0x00f1);
  check('来源归属：语音释放后其它设备可重新成为来源',
    ctx.activeSource === 'other-source' && foreignAfterVoice.ptr.buf[3] === 0);
  ctx.handleCommand(JSON.stringify({ type: 'synth', off: true }));
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 11, report: [], clear: [] }));
}

/* -------- 14. 语音键 clear-only：F5 必须在报告层被清掉，不能漏到桌面（2026-11-12 报障） -------- */
{
  await freshStubConnection();
  ctx.handleCommand('{"type":"renew"}');
  ctx.handleCommand('{"type":"mode","clear":true}');
  ctx.handleCommand('{"type":"gate","on":false}');
  ctx.handleCommand(JSON.stringify({ type: 'synth', off: true }));
  ctx.handleCommand(JSON.stringify({
    type: 'targets', generation: 12, report: [0x00f1], clear: [0x00f1, 0x003e],
  }));
  ctx.activeSource = null;
  ctx.pressed = {};

  const beforeCanary = ctx.stat.canary_hits;
  const beforeTarget = ctx.stat.target_hits;
  const beforeVoiceClear = ctx.stat.voice_clear_hits || 0;
  const plainVoice = press(0x003e);
  check('语音键 clear-only：槽被清零（F5 不交给 Windows）',
    plainVoice.ptr.buf[3] === 0 && plainVoice.ptr.buf[4] === 0 && plainVoice.self.cleared === true,
    `buf3=0x${plainVoice.ptr.buf[3].toString(16)} cleared=${plainVoice.self.cleared}`);
  check('语音键 clear-only：不计入 canary_hits（不是哨兵键）',
    ctx.stat.canary_hits === beforeCanary,
    `canary=${ctx.stat.canary_hits} before=${beforeCanary}`);
  check('语音键 clear-only：voice_clear_hits +1',
    (ctx.stat.voice_clear_hits || 0) === beforeVoiceClear + 1,
    `voice_clear=${ctx.stat.voice_clear_hits}`);
  check('语音键 clear-only：不计入 target_hits（不产生映射边沿）',
    ctx.stat.target_hits === beforeTarget);

  /* 合成配置存在时，槽内替换优先于清空：语音键仍变成 RightAlt，且不污染 canary。 */
  ctx.handleCommand(JSON.stringify({ type: 'synth', from: 0x003e, to: 0x00e6 }));
  const beforeSynthCanary = ctx.stat.canary_hits;
  const beforeSynthVoiceClear = ctx.stat.voice_clear_hits || 0;
  const synthVoice = press(0x003e);
  check('语音键 synth + clear-only：仍替换为 0x00E6',
    synthVoice.ptr.buf[3] === 0xe6 && synthVoice.ptr.buf[4] === 0x00,
    `buf3=0x${synthVoice.ptr.buf[3].toString(16)}`);
  check('语音键 synth + clear-only：不计入 canary_hits',
    ctx.stat.canary_hits === beforeSynthCanary);
  check('语音键 synth + clear-only：不重复计 voice_clear_hits',
    (ctx.stat.voice_clear_hits || 0) === beforeSynthVoiceClear);
  ctx.handleCommand(JSON.stringify({ type: 'synth', off: true }));
  ctx.handleCommand(JSON.stringify({ type: 'targets', generation: 13, report: [], clear: [] }));
}

/* -------- 15. 增强捕获关闭：语音键必须恢复原生（不能把 clear-only 变成常驻吞键） -------- */
{
  ctx.handleCommand(JSON.stringify({ type: 'synth', off: true }));
  ctx.handleCommand(JSON.stringify({
    type: 'targets', generation: 14, report: [], clear: [],
  }));
  ctx.activeSource = null;
  ctx.pressed = {};

  const beforeVoiceClear = ctx.stat.voice_clear_hits || 0;
  const { ptr, self } = press(0x003e);
  check('增强捕获关闭：语音键不被清空（F5 恢复原生）',
    ptr.buf[3] === 0x3e && !self.cleared,
    `buf3=0x${ptr.buf[3].toString(16)} cleared=${self.cleared}`);
  check('增强捕获关闭：voice_clear_hits 不增长',
    (ctx.stat.voice_clear_hits || 0) === beforeVoiceClear,
    `voice_clear=${ctx.stat.voice_clear_hits}`);
}

const failed = results.filter((r) => !r.ok);
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'}  ${r.name}${r.detail ? '  -- ' + r.detail : ''}`);
}
console.log(`\n${results.length - failed.length}/${results.length} 通过`);
process.exit(failed.length === 0 ? 0 : 1);
