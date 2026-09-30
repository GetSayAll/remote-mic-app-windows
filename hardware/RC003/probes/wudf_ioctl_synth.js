'use strict';
/*
 * RC003 报告层**合成按键**探针 —— 验证能否产出「不带 LLKHF_INJECTED」的按键。
 *
 * 与 wudf_ioctl_write.js 的关系
 * -----------------------------
 * 本文件的结构几乎逐行继承自 `wudf_ioctl_write.js`（那份已在真机上跑过 7 项
 * PASS，含 "改写被翻译层采纳" 的证明）。差异只有两点：
 *
 *   1. **TARGET_USAGES 变成可配置**（`__TARGET_USAGES_JSON__`）。原版硬编码
 *      三键 `0xF1/0x80/0x81`，本版默认取 `0x004A`（主页）——理由见下。
 *   2. 语义从"拦截/擦除"扩展为"替换"，于是 substitute 可以是**修饰键类 usage**
 *      （Keyboard page 0x07 的 `0x00E0`–`0x00E7`），例如：
 *         0x00E2 = Keyboard RightAlt   -> kbdhid 翻成 VK_RMENU（豆包长按/一次唤起的键）
 *         0x00E0 = Keyboard LeftCtrl   ┐
 *         0x00E3 = Keyboard LeftGUI    ┘ 一起就是微信 WeType 的 Ctrl+Win 和弦
 *
 * 为什么默认拿主页键 0x004A 做载体
 * --------------------------------
 * 三条硬理由，都是为了避免"判据读不出差别"：
 *   - 主页键在 Windows 侧**有可见后果**（焦点在文本里时光标跳到行首），
 *     因此"它不见了"本身就是一个有分辨力的观测；
 *   - 它的 HID 报告**确定存在**（与三键不同，三键被 kbdhid 丢弃，外部永远
 *     零事件，用它们做载体会让"生效/没生效"看起来一模一样）；
 *   - 它的释放报告（usage 槽归零）确定存在，可用于验证**边沿配对**。
 * 这与既有 probe 里"必须带阳性对照"的教训同源：不能选一个本来就无声的键。
 *
 * 报告布局（取自只读实测日志，不是推断）
 * ------------------------------------
 *     偏移 0   report_id  = 0x01
 *     偏移 1   modifiers
 *     偏移 2   reserved
 *     偏移 3/5/7  三个 16 位小端 usage 槽（本文件只允许写这 6 个字节）
 *
 * 修饰键为什么放在 usage 槽而不是 modifiers 字节
 * ---------------------------------------------
 * 标准键盘描述符里 Alt/Ctrl/GUI 通常是字节 1 的位图。但在这里**不碰 modifiers**
 * 是有意的：① 现有安全护栏就是"只写偏移 3..8"，不去打破一条已经在真机验证过的
 * 约束；② 我们无法确认 RC003 描述符的第 1 字节真是标准的 modifier 位图
 * （它可能另有含义），写错位的后果比写一个未被映射的 usage 更不可控。
 * 若 usage 槽路线被证明不被翻译，**再**考虑动 modifiers——那是下一轮的事。
 *
 * 三种模式（由计划表在本地时钟上切换，**不依赖反向通道**）
 * --------------------------------------------------
 *     observe        只观察，一个字节都不写
 *     rewrite        目标 usage -> substitute（本版主要用法）
 *     erase-target   仅把目标 usage 清零
 *     erase-all      三个 usage 槽全部清零
 *
 * 零残留设计
 * ----------
 * `onLeave` 在内核没有同时改写过缓冲区的前提下把原始字节写回，既不影响本次
 * 调用已完成的语义，又不给宿主留下被篡改的缓冲区（若被改写则如实上报
 * `kernel_changed=true` 并放弃回写）。
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
 * 真实运行中 Python -> JS 的 post()/recv() 只送达第一条消息，JS -> Python 的
 * send() 全程正常。因此这里**完全没有反向通道**：计划表随源码注入，阶段由
 * `Date.now()` 本地推进；上行只用一个 send()。
 *
 * 脱敏：不打印指针绝对值，只打印模块名+偏移。
 */

var TARGET_IOCTL = 0x80018483;
var TARGET_USAGES = __TARGET_USAGES_JSON__;   /* { usage: "可读名" } */
var USAGE_NAMES = __USAGE_NAMES_JSON__;
var SCHEDULE = __SCHEDULE_JSON__;
var BIND_PHASE = __BIND_PHASE__;
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

/* ---- 设备维度绑定 ----
   `handleIds`: 句柄 -> 编号（h0/h1…，按首次出现顺序，不打印句柄值本身）
   `handleHits`: 句柄 -> 在绑定阶段出现"含目标 usage 报告"的次数
   `boundHandle`: 绑定阶段命中数最多的那个句柄；改写阶段只作用于它 */
var handleIds = {};
var handleHits = {};
var handleSeq = 0;
var boundHandle = null;
var boundHits = 0;

function handleKeyOf(ptr) {
  try {
    return String(ptr);
  } catch (e) {
    return 'unknown';
  }
}

function handleId(key) {
  if (!(key in handleIds)) {
    handleIds[key] = 'h' + handleSeq;
    handleSeq++;
  }
  return handleIds[key];
}

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

function usageName(u) {
  if (u in USAGE_NAMES) return '0x' + ('0000' + u.toString(16)).slice(-4) + '(' + USAGE_NAMES[u] + ')';
  return '0x' + ('0000' + u.toString(16)).slice(-4);
}

/* 报告里出现了哪些目标 usage（只读，不改字节）。 */
function targetsIn(bytes) {
  var slots = [3, 5, 7];
  var found = [];
  for (var i = 0; i < slots.length; i++) {
    var o = slots[i];
    var u = bytes[o] | (bytes[o + 1] << 8);
    if ((u in TARGET_USAGES) && found.indexOf(u) < 0) found.push(u);
  }
  return found;
}

/* 只在偏移 3..8 上工作；返回 {changed, bytes, touched}。
   `touched` 记录被改的槽的原始 usage，供上层区分按下沿与释放沿：
   substitute 类 usage 只有在**报告里出现目标 usage** 时才会产出，
   usage 槽归零（释放报告）时不参与改与——原样放行，由 kbdhid 判定 Alt 抬起。 */
function planMutation(bytes, mode, substitute) {
  var out = bytes.slice();
  var slots = [3, 5, 7];
  var changed = false;
  var touched = [];
  for (var i = 0; i < slots.length; i++) {
    var o = slots[i];
    var u = out[o] | (out[o + 1] << 8);
    if (mode === 'erase-all') {
      if (u !== 0) {
        out[o] = 0;
        out[o + 1] = 0;
        changed = true;
        touched.push(u);
      }
      continue;
    }
    if (!(u in TARGET_USAGES)) continue;
    touched.push(u);
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
  return { changed: changed, bytes: out, touched: touched };
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

      var hKey = handleKeyOf(args[0]);
      this.handleId = handleId(hKey);

      var inHex = dump(this.inPtr, this.inLen);
      var outHex = dump(this.outPtr, this.outLen);
      this.origHex = outHex;
      this.mutated = false;
      this.newHex = outHex;
      this.writePath = '';
      this.writeErr = '';
      this.restoreResult = '';
      this.touched = [];

      if (recordsSent < MAX_RECORDS) recordsSent++;

      var minishape = outHex.length >= 18 && outHex.substr(0, 2) === '01';
      this.shape = minishape;

      /* 绑定阶段：统计"含目标 usage 的键盘报告"来自哪个句柄。
         只在这里统计——用户在此期间只按遥控器，因此主导句柄一定是它。 */
      if (info.name === BIND_PHASE && minishape) {
        var probeBytes = [];
        for (var pb = 0; pb < 9; pb++) {
          probeBytes.push(parseInt(outHex.substr(pb * 2, 2), 16));
        }
        if (targetsIn(probeBytes).length > 0) {
          handleHits[hKey] = (handleHits[hKey] || 0) + 1;
          if (handleHits[hKey] > boundHits) {
            boundHits = handleHits[hKey];
            boundHandle = hKey;
          }
        }
      }

      /* ---- 写入闸门 ---- */
      var guardOk =
        !disarmed &&
        mutating(this.mode) &&
        boundHandle !== null && hKey === boundHandle &&
        this.outLen >= 9 &&
        this.outPtr !== null && !this.outPtr.isNull() &&
        (this.t - t0) <= TOTAL_MS + GRACE_MS &&
        mutations < MAX_MUTATIONS &&
        this.inLen === 8 &&
        inHex.length >= 12 && inHex.substr(8, 2) === '02' && inHex.substr(10, 2) === '01' &&
        minishape;

      if (guardOk) {
        var bytes = [];
        for (var i = 0; i < 9; i++) bytes.push(parseInt(outHex.substr(i * 2, 2), 16));
        var plan = planMutation(bytes, this.mode, info.substitute);
        this.touched = plan.touched;
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
        /* 已进入改写阶段但闸门未通过——记录原因，避免"看起来什么都没发生"。
           `no_bound_handle` / `other_device` 是新增的两条：前者说明绑定阶段
           没抓到任何含目标 usage 的报告（用户没按、或设备没连），后者说明
           这份报告来自同一宿主里的**其它设备**，按设计应当放行不动。 */
        this.writeErr = disarmed ? 'disarmed'
          : (boundHandle === null ? 'no_bound_handle'
            : (hKey !== boundHandle ? 'other_device'
              : (mutations >= MAX_MUTATIONS ? 'mutation_cap'
                : (this.inLen !== 8 ? 'in_len' : 'guard'))));
      }

      send({
        type: 'ioctl',
        t: this.t,
        dir: 'enter',
        phase: this.phase,
        idx: this.phaseIdx,
        mode: this.mode,
        substitute: info.substitute,
        handle_id: this.handleId,
        handle_bound: (boundHandle !== null && hKey === boundHandle),
        in_len: this.inLen,
        out_len: this.outLen,
        in_hex: inHex,
        orig_hex: outHex,
        new_hex: this.newHex,
        mutated: this.mutated,
        touched: this.touched,
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
  bind_phase: BIND_PHASE,
  target_usages: Object.keys(TARGET_USAGES).map(function (k) {
    return { usage: parseInt(k, 10), name: TARGET_USAGES[k] };
  }),
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
    bound_handle: boundHandle === null ? null : handleId(boundHandle),
    bound_hits: boundHits,
    write_errors: writeErrors
  });
}, HEARTBEAT_MS);
