'use strict';
/*
 * RC003 WUDF 宿主 IOCTL **写入**（拦截）探针 —— 最小实验（Frida agent）。
 *
 * 与只读版（wudf_ioctl_tap.js）的关系
 * -----------------------------------
 * 只读版已证明：本机 RC003 的 9 字节键盘报告在 `onEnter` 时就已由宿主填好，
 * `IOCTL 0x80018483`（in 8 / out 9，operation/selector `02 01`）的输出缓冲区
 * 里确实存在 `0x00F1` / `0x0080` / `0x0081`，且 `onLeave` 时内容 0/30 无变化。
 * 本文件把它推进到**写入**：在 `onEnter` 改写 / 擦除 usage 槽，验证改写是否
 * 被下游（Windows 键码翻译）采纳。
 *
 * 报告布局（取自只读实测日志，不是推断）
 * ------------------------------------
 *     偏移 0   report_id  = 0x01
 *     偏移 1   modifiers
 *     偏移 2   reserved
 *     偏移 3/5/7  三个 16 位小端 usage 槽（本文件只允许写这 6 个字节）
 *
 * 三种模式（由内建计划表在本地时钟上切换，**不依赖反向通道**）
 * ----------------------------------------------------------
 *     observe      只观察，一个字节都不写
 *     rewrite      目标 usage -> substitute（默认 0x0028=确定，映射已被实测证明；
 *                                        可用 0x0068=F13 取得无歧义证据）
 *     erase-target 仅把目标 usage 清零（保留确定/主页等非目标键）
 *     erase-all    三个 usage 槽全部清零（参考实现的形态，必然连带确定/主页）
 *
 * 零残留设计
 * ----------
 * 即使缓冲区是每次调用独立的临时内存，本探针仍在 `onLeave` **把原始字节写回**，
 * 前提是内核没有在调用期间改写过它（若被改写则放弃回写并如实上报
 * `kernel_changed=true`）。这样既不影响本次调用已完成的语义，又不给宿主留下
 * 被篡改的缓冲区。
 *
 * 硬性安全闸（任何一条不满足就绝不写）
 * ----------------------------------
 *     1. IoControlCode == 0x80018483
 *     2. InBufferLength == 8 且 OutputBufferLength >= 9
 *     3. OutputBuffer != NULL 且 out[0] == 0x01（键盘报告）
 *     4. in[4] == 0x02 且 in[5] == 0x01（operation / selector）
 *     5. 当前阶段 mode 属于改写类，且未越过计划表末尾 + 宽限期
 *     6. 累计写入次数 < MAX_MUTATIONS
 * 只写偏移 3..8，绝不触碰 report_id / modifiers / reserved。
 *
 * 通道设计（沿用只读版踩坑结论）
 * ----------------------------
 * 真实运行中 Python -> JS 的 post()/recv() 只送达第一条消息（session 0 目标），
 * 同一次运行的 JS -> Python send() 全程正常。因此这里**完全没有反向通道**：
 * 计划表随源码注入，阶段由 `Date.now()` 本地推进；上行只用一个 send()。
 *
 * 脱敏：不打印指针绝对值，只打印模块名+偏移。
 */

var TARGET_IOCTL = 0x80018483;
var TARGET_USAGES = { 0xF1: '返回', 0x80: '音量+', 0x81: '音量-' };
var SCHEDULE = __SCHEDULE_JSON__;
var MAX_MUTATIONS = 400;
var MAX_RECORDS = 2000;
var GRACE_MS = 3000;      /* 计划表走完后继续观察的宽限期，之后永久解除武装 */
var HEARTBEAT_MS = 1000;
var MAX_DUMP = 32;

var TOTAL_MS = 0;
for (var si = 0; si < SCHEDULE.length; si++) TOTAL_MS += SCHEDULE[si].ms;

var t0 = Date.now();
var totalCalls = 0;
var targetCalls = 0;
var recordsSent = 0;
var mutations = 0;
var writeOk = 0;
var writeFail = 0;
var restored = 0;
var kernelChanged = 0;
var disarmed = false;
var writeErrors = {};

function toHex(u8) {
  var s = '';
  for (var i = 0; i < u8.length; i++) {
    var h = u8[i].toString(16);
    s += h.length === 1 ? '0' + h : h;
  }
  return s;
}

function dump(ptr, len) {
  if (len <= 0 || ptr === null || ptr.isNull()) return '';
  var n = Math.min(len, MAX_DUMP);
  try {
    var buf = ptr.readByteArray(n);
    if (buf === null) return '<null>';
    return toHex(new Uint8Array(buf));
  } catch (e) {
    return '<unreadable>';
  }
}

function callerOf(addr) {
  try {
    var m = Process.findModuleByAddress(addr);
    if (m === null) return 'unknown';
    return m.name + '+0x' + addr.sub(m.base).toString(16);
  } catch (e) {
    return 'error';
  }
}

function phaseAt(elapsed) {
  var acc = 0;
  for (var i = 0; i < SCHEDULE.length; i++) {
    acc += SCHEDULE[i].ms;
    if (elapsed < acc) return i;
  }
  return SCHEDULE.length; /* 计划表之外 */
}

function phaseInfo(idx) {
  if (idx < 0 || idx >= SCHEDULE.length) {
    return { name: 'after', mode: 'observe', substitute: 0, idx: idx };
  }
  var p = SCHEDULE[idx];
  return {
    name: String(p.name),
    mode: String(p.mode),
    substitute: p.substitute === undefined ? 0 : (p.substitute & 0xFFFF),
    idx: idx
  };
}

function mutating(mode) {
  return mode === 'rewrite' || mode === 'erase-target' || mode === 'erase-all';
}

function hex2(v) {
  var h = (v & 0xFF).toString(16);
  return h.length === 1 ? '0' + h : h;
}

/* 只在偏移 3..8 上工作；返回 {changed, bytes}（bytes 为 9 个数字的数组） */
function planMutation(bytes, mode, substitute) {
  var out = bytes.slice();
  var slots = [3, 5, 7];
  var changed = false;
  for (var i = 0; i < slots.length; i++) {
    var o = slots[i];
    var u = out[o] | (out[o + 1] << 8);
    if (mode === 'erase-all') {
      if (u !== 0) {
        out[o] = 0;
        out[o + 1] = 0;
        changed = true;
      }
      continue;
    }
    if (!(u in TARGET_USAGES)) continue;
    if (mode === 'rewrite') {
      if (u === substitute) continue;
      out[o] = substitute & 0xFF;
      out[o + 1] = (substitute >> 8) & 0xFF;
      changed = true;
    } else { /* erase-target */
      out[o] = 0;
      out[o + 1] = 0;
      changed = true;
    }
  }
  return { changed: changed, bytes: out };
}

/* 写入 9 字节；先直写，失败再尝试放开页保护。返回 {ok, path, err} */
function writeReport(ptr, bytes) {
  var ab = new Uint8Array(bytes).buffer;
  try {
    ptr.writeByteArray(ab);
    return { ok: true, path: 'direct', err: null };
  } catch (e1) {
    try {
      Memory.protect(ptr, 9, 'rw-');
      ptr.writeByteArray(new Uint8Array(bytes).buffer);
      return { ok: true, path: 'protect', err: null };
    } catch (e2) {
      return { ok: false, path: 'none', err: String(e1) + ' / ' + String(e2) };
    }
  }
}

var ntdll = Process.getModuleByName('ntdll.dll');
var target = ntdll.getExportByName('NtDeviceIoControlFile');

Interceptor.attach(target, {
  onEnter: function (args) {
    try {
      totalCalls++;
      var code = args[5].toUInt32();
      this.hit = (code === TARGET_IOCTL);
      if (!this.hit) return;

      targetCalls++;
      this.t = Date.now();
      var info = phaseInfo(phaseAt(this.t - t0));
      this.phase = info.name;
      this.phaseIdx = info.idx;
      this.mode = info.mode;

      this.inPtr = args[6];
      this.inLen = args[7].toUInt32();
      this.outPtr = args[8];
      this.outLen = args[9].toUInt32();
      this.caller = callerOf(this.returnAddress);

      var inHex = dump(this.inPtr, this.inLen);
      var outHex = dump(this.outPtr, this.outLen);
      this.origHex = outHex;
      this.mutated = false;
      this.newHex = outHex;
      this.writePath = '';
      this.writeErr = '';
      this.restoreResult = '';

      if (recordsSent < MAX_RECORDS) recordsSent++;

      /* ---- 写入闸门 ---- */
      var guardOk =
        !disarmed &&
        mutating(this.mode) &&
        this.outLen >= 9 &&
        this.outPtr !== null && !this.outPtr.isNull() &&
        (this.t - t0) <= TOTAL_MS + GRACE_MS &&
        mutations < MAX_MUTATIONS &&
        this.inLen === 8 &&
        inHex.length >= 12 && inHex.substr(8, 2) === '02' && inHex.substr(10, 2) === '01' &&
        outHex.length >= 18 && outHex.substr(0, 2) === '01';

      if (guardOk) {
        var bytes = [];
        for (var i = 0; i < 9; i++) bytes.push(parseInt(outHex.substr(i * 2, 2), 16));
        var plan = planMutation(bytes, this.mode, info.substitute);
        if (plan.changed) {
          var res = writeReport(this.outPtr, plan.bytes);
          mutations++;
          if (res.ok) {
            var newHex = toHex(new Uint8Array(plan.bytes));
            var readBack = dump(this.outPtr, this.outLen);
            if (readBack === newHex) {
              writeOk++;
              this.mutated = true;
              this.newHex = newHex;
              this.writePath = res.path;
            } else {
              writeFail++;
              this.writeErr = 'readback_mismatch:' + readBack;
            }
          } else {
            writeFail++;
            var key = res.err ? res.err.slice(0, 60) : 'unknown';
            writeErrors[key] = (writeErrors[key] || 0) + 1;
            this.writeErr = res.err || 'unknown';
          }
        }
      } else if (mutating(this.mode) && this.outLen >= 9 && outHex.length >= 18) {
        /* 已进入改写阶段但闸门未通过——记录原因，避免"看起来什么都没发生" */
        this.writeErr = disarmed ? 'disarmed'
          : (mutations >= MAX_MUTATIONS ? 'mutation_cap'
            : (this.inLen !== 8 ? 'in_len' : 'guard'));
      }

      send({
        type: 'ioctl',
        t: this.t,
        dir: 'enter',
        phase: this.phase,
        idx: this.phaseIdx,
        mode: this.mode,
        in_len: this.inLen,
        out_len: this.outLen,
        in_hex: inHex,
        orig_hex: outHex,
        new_hex: this.newHex,
        mutated: this.mutated,
        write_path: this.writePath,
        write_err: this.writeErr,
        caller: this.caller
      });
    } catch (e) {
      send({ type: 'error', where: 'onEnter', msg: String(e) });
    }
  },
  onLeave: function (retval) {
    try {
      if (!this.hit || !this.mutated) return;
      var cur = dump(this.outPtr, this.outLen);
      var detail = '';
      if (cur === this.newHex) {
        var orig = [];
        for (var i = 0; i < this.origHex.length / 2; i++) {
          orig.push(parseInt(this.origHex.substr(i * 2, 2), 16));
        }
        var res = writeReport(this.outPtr, orig);
        var back = dump(this.outPtr, this.outLen);
        if (res.ok && back === this.origHex) {
          restored++;
          detail = 'restored';
        } else {
          detail = 'restore_failed:' + back;
        }
      } else {
        kernelChanged++;
        detail = 'kernel_changed:' + cur;
      }
      send({
        type: 'ioctl',
        t: this.t,
        dir: 'leave',
        phase: this.phase,
        idx: this.phaseIdx,
        mode: this.mode,
        ret: retval.toInt32(),
        out: cur,
        detail: detail
      });
    } catch (e) {
      send({ type: 'error', where: 'onLeave', msg: String(e) });
    }
  }
});

send({
  type: 'ready',
  t: t0,
  target_ioctl: ('00000000' + TARGET_IOCTL.toString(16)).slice(-8),
  hook_ok: !target.isNull(),
  total_ms: TOTAL_MS,
  max_mutations: MAX_MUTATIONS,
  schedule: SCHEDULE.map(function (p) {
    return { name: p.name, ms: p.ms, mode: p.mode,
             substitute: p.substitute === undefined ? 0 : p.substitute };
  })
});

setInterval(function () {
  var elapsed = Date.now() - t0;
  var info = phaseInfo(phaseAt(elapsed));
  if (!disarmed && elapsed > TOTAL_MS + GRACE_MS) disarmed = true;
  send({
    type: 'hb',
    t: Date.now(),
    elapsed: elapsed,
    idx: info.idx,
    phase: info.name,
    mode: disarmed ? 'observe(disarmed)' : info.mode,
    total_calls: totalCalls,
    target_calls: targetCalls,
    records_sent: recordsSent,
    mutations: mutations,
    write_ok: writeOk,
    write_fail: writeFail,
    restored: restored,
    kernel_changed: kernelChanged,
    disarmed: disarmed,
    write_errors: writeErrors
  });
}, HEARTBEAT_MS);
