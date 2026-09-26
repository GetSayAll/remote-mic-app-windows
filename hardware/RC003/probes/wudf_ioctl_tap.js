'use strict';
/*
 * RC003 WUDF 宿主 IOCTL 只读探针（Frida agent）。
 *
 * 目的：在承载 RC003 的 WUDFHost.exe 内，挂 ntdll!NtDeviceIoControlFile，
 * 把 UMDF 输出复制入口（IoControlCode = 0x80018483，in 8 / out 9）的
 * 输入/输出缓冲区原样转储出来，直接回答"本机 RC003 的 9 字节键盘报告里
 * 到底有没有 0x00F1 / 0x0080 / 0x0081"。
 *
 * 只读契约（务必保持）：
 *   - 不写任何目标缓冲区；不调用 DeviceIoControl；不 SendInput；
 *   - 只调用 readByteArray（Frida 内部走安全读，越界不抛硬件异常）；
 *   - 不改变报告流时序，因此对系统无功能影响。
 *
 * 与参考实现（ZSTDJan/windows-remote-mic-app）的差别：参考实现在 onEnter
 * 清空 6 字节 usage 槽；本探针在 onEnter / onLeave 各转储一次，用于独立
 * 验证"报告在调用前已由宿主填好、内核在调用期间复制"这一时序结论。
 *
 * 通道设计（2026-09-23 踩坑后定稿）：**只使用单向 send()**。
 * 真实运行中 Python -> JS 的 post()/recv() 只送达了第一条消息
 * （phase 停在 pre、收尾汇总为空），而同一次运行里 JS -> Python 的 send()
 * 全程正常。自检脚本 `frida_msg_selftest.py` 显示 post/recv 在 spawn 与
 * attach 两种模式下都可用，因此该异常归因于 session 0 目标环境，原因未定位。
 * 结论：**阶段归属与统计一律不由反向通道承担**——
 *   - 每条记录自带 t（Date.now()），Python 侧按自己的时间轴归属阶段；
 *   - 累计统计随每次心跳（setInterval）随 send() 上行，最后一次心跳即终值。
 *
 * 脱敏：不打印任何指针绝对值，只打印 模块名+偏移。
 */

var TARGET_IOCTL = 0x80018483;
var MAX_DUMP = 32;
var MAX_RECORDS = 1200;
var TICK_MS = 3000;

var totalCalls = 0;
var targetCalls = 0;
var recordsSent = 0;
var truncated = false;
var ioctlCensus = {};
var outLengthCensus = {};
var inLengthCensus = {};
var usageHist = {};
var inSelectorCensus = {};

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

function bump(map, key) {
  map[key] = (map[key] || 0) + 1;
}

/* 统计非零的 16 位 usage 槽；usage 名一律由 Python 侧渲染，这里只出数字。 */
function tallyUsages(outHex) {
  if (outHex.length < 18) return;
  var b = [];
  for (var i = 0; i < 9; i++) b.push(parseInt(outHex.substr(i * 2, 2), 16));
  if (b[0] !== 1) return;
  var offs = [3, 5, 7];
  for (var k = 0; k < offs.length; k++) {
    var u = b[offs[k]] | (b[offs[k] + 1] << 8);
    if (u === 0) continue;
    var key = ('0000' + u.toString(16)).slice(-4);
    usageHist[key] = (usageHist[key] || 0) + 1;
  }
}

function stats() {
  return {
    total_calls: totalCalls,
    target_calls: targetCalls,
    records_sent: recordsSent,
    truncated: truncated,
    ioctl_census: ioctlCensus,
    in_length_census: inLengthCensus,
    out_length_census: outLengthCensus,
    in_selector_census: inSelectorCensus,
    usage_hist: usageHist
  };
}

var ntdll = Process.getModuleByName('ntdll.dll');
var target = ntdll.getExportByName('NtDeviceIoControlFile');

Interceptor.attach(target, {
  onEnter: function (args) {
    try {
      totalCalls++;
      var code = args[5].toUInt32();
      bump(ioctlCensus, ('00000000' + code.toString(16)).slice(-8));

      this.hit = (code === TARGET_IOCTL);
      if (!this.hit) return;

      targetCalls++;
      this.t = Date.now();
      this.inPtr = args[6];
      this.inLen = args[7].toUInt32();
      this.outPtr = args[8];
      this.outLen = args[9].toUInt32();
      this.caller = callerOf(this.returnAddress);

      bump(inLengthCensus, String(this.inLen));
      bump(outLengthCensus, String(this.outLen));

      var inHex = dump(this.inPtr, this.inLen);
      var outHex = dump(this.outPtr, this.outLen);
      this.enterOut = outHex;

      /* 参考实现判据：input[4] == 2（operation）、input[5] == 1（selector）。
         直接统计实际取值，便于异机比对。 */
      if (inHex.length >= 12) {
        bump(inSelectorCensus, inHex.substr(8, 2) + '/' + inHex.substr(10, 2));
      }
      tallyUsages(outHex);

      if (recordsSent < MAX_RECORDS) {
        recordsSent++;
        send({
          type: 'ioctl',
          t: this.t,
          dir: 'enter',
          in_len: this.inLen,
          out_len: this.outLen,
          in_hex: inHex,
          out_hex: outHex,
          caller: this.caller
        });
      } else {
        truncated = true;
      }
    } catch (e) {
      send({ type: 'error', where: 'onEnter', msg: String(e) });
    }
  },
  onLeave: function (retval) {
    try {
      if (!this.hit) return;
      var outHex = dump(this.outPtr, this.outLen);
      if (recordsSent < MAX_RECORDS) {
        recordsSent++;
        send({
          type: 'ioctl',
          t: this.t,
          dir: 'leave',
          ret: retval.toInt32(),
          in_len: this.inLen,
          out_len: this.outLen,
          out_hex: outHex,
          out_changed: (outHex !== this.enterOut),
          caller: this.caller
        });
      } else {
        truncated = true;
      }
    } catch (e) {
      send({ type: 'error', where: 'onLeave', msg: String(e) });
    }
  }
});

send({
  type: 'ready',
  t: Date.now(),
  target_ioctl: ('00000000' + TARGET_IOCTL.toString(16)).slice(-8),
  hook_ok: !target.isNull(),
  tick_ms: TICK_MS
});

setInterval(function () {
  var s = stats();
  s.type = 'stats';
  s.t = Date.now();
  send(s);
}, TICK_MS);
