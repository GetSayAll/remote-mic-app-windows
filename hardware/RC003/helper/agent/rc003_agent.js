'use strict';
/*
 * rc003_agent.js —— RC003 增强捕获轨（路线 A）的 Gadget 侧 agent。
 * 下文实测叙述来自固定上游提交的历史证据，不代表当前本地版本已通过真机验收。
 *
 * 定位
 * ----
 * 由 Frida Gadget 以 `interaction.type = "script"` 的方式加载进承载 RC003 的
 * WUDFHost.exe。职责单一：在 `ntdll!NtDeviceIoControlFile` 的 UMDF 输出复制入口
 * **只拦截主程序动态下发的目标 usage**，把边沿通过 loopback TCP 上报给助手，
 * 未配置按键的报告一字节不动。语音键不在白名单内，继续由 ATVV 会话处理。
 *
 * 与既有实验探针（wudf_ioctl_write.js）的差别
 * -------------------------------------------
 * 1. **选择性**：探针会 `erase-all`；本 agent 只清主程序动态下发的已映射按键，
 *    未配置按键仍走 Windows 原生路径。
 * 2. **传输**：探针用 `send()`（需要 host 侧 frida 客户端）；本 agent 用
 *    loopback TCP，助手是纯 Rust 进程，**不需要 frida 客户端库**。
 * 3. **租约**：探针靠内建计划表；本 agent 靠助手每 500ms 的续约，
 *    续约一停就停止清键（fail-open）。
 *
 * Frida 17 Socket API 的实测语义（不要按直觉改）
 * ----------------------------------------------
 * 以下是 2026-09-23 在本机 frida 17.18.0 上逐条实测的结论，其中多条与直觉相反：
 *   - `Socket.listen/connect` 返回 **Promise**，不是连接对象。
 *   - 连接对象没有 `write`/`read`，只有 `.input` / `.output` 两个流。
 *   - `output.write()` **只接受 ArrayBuffer / TypedArray**；传字符串会被
 *     **静默丢弃**（不抛错、零字节送达）。
 *   - `output.write()` **返回 Promise，而且同一时刻只允许一个在飞**（2026-10-02 实测，
 *     `agent_selftest.py` 用例 F 钉住；探针输出见该用例注释）：上一次没 settle 时
 *     的第二次 write **既不抛错也不排队**，而是立刻以
 *     `Error: stream has outstanding operation` **reject** —— **零字节送达**。
 *     火并忘（fire-and-forget）的写法完全看不到这个 reject ⇒ **一次回调里连写多行，
 *     只有第一行到得了对端**。真机现象正是这一类：`targets_ack`、`targets:applied`、
 *     `synth_ack`、`synth:frame` 整类消失，而定时器每 tick 只写一行的 `hb` 一直正常。
 *     对策见 `sendLine` / `txPump`：出站队列 + 串行冲刷，由 Promise settle 驱动。
 *   - 对端关闭后继续 `output.write()` **不抛错**（实测 3/3），
 *     所以"写失败"**不能**用来检测助手死亡。
 *   - `Socket.connect` 到已关闭端口：1.5s 窗口内不 settle；延长观察后约 **2.2s**
 *     以 reject 结束（ECONNREFUSED）。结论不变——**太慢且语义依赖平台**，
 *     不适合当探活机制；租约（毫秒级、只看"续约有没有来"）才是正解。
 *     **补充（2026-09-23 真机）**：2.2s > 1s 心跳周期 ⇒ 助手缺席期间 connect 会累积，
 *     助手一回来就有 2~3 个 connect 同时成功、互相覆盖 `sock`，被覆盖的那些会被 GC
 *     终结并发出 RST。所以 `connectOnce` 必须有 `attemptSeq`/`connecting` 并发守卫。
 *   - 挂起的 `input.read()` 在对端关闭时**不会**以 EOF 结束（1.3s 内未 settle）。
 *   - `input.read(n)` 返回 Promise，有数据时 resolve 一个正好 n 字节以内的 buffer。
 * ⇒ 结论：数据通道双向可用，但**没有任何可靠的探活原语**。
 *   因此清键许可靠**租约**判定（`Date.now() - lastRenewAt < LEASE_MS`），
 *   而不是靠连接状态推断。助手死亡 ⇒ 续约停止 ⇒ 最迟 LEASE_MS 后自动停止清键。
 *
 * 安全感（任何一条不满足就绝不写目标缓冲区）
 * ----------------------------------------
 *   1. IoControlCode == 0x80018483
 *   2. InBufferLength == 8 且 out 长度 >= 9
 *   3. OutputBuffer 非空 且 out[0] == 0x01（键盘报告）
 *   4. in[4] == 0x02 且 in[5] == 0x01（operation / selector）
 *   5. 租约有效（助手续约未过期）
 *   6. mode == 'clear'（观察模式一个字节都不写）
 *   7. 累计清键次数 < MAX_CLEARS
 *   8. usage ∈ clearUsages（产品目标来自主程序；验收时可能多一个哨兵键）
 * 只写偏移 3..8，绝不触碰 report_id / modifiers / reserved。
 *
 * ⚠️ 一个必须说明的可观测性缺口（2026-09-23）
 * ------------------------------------------
 *   RC003 的三键在 Windows 侧**本来就零事件**（kbdhid 丢掉了这三个 usage）。
 *   所以"清掉"与"不清"在外部看不出差别 —— 音量不变、也不会有字符。
 *   即以"无原生动作"为判据的验收在 RC003 上是**恒为真**的，没有分辨力。
 *   要证明清空真的作用到了报告上，必须清一个 Windows 本来可用的键：
 *   助手侧 `--canary-usage 0x4A`（主页）会通过 targets 命令把它加进 clearUsages。
 *   届时"按主页无反应 / 助手退出后恢复"才是"拦截生效 / fail-open"的**直接**证据。
 *
 * 协议（换行分隔的 JSON，纯 ASCII，两个方向都是）
 * ---------------------------------------------
 *   agent -> helper : {"type":"hello","instance":"...",...} / {"type":"hb",...} / {"type":"edge",...}
 *                     / {"type":"log","msg":"..."} / {"type":"bye"}
 *                     / {"type":"stopped","instance":"...","stop_id":"...",
 *                        "hook_detached":true,"released_all":true}
 *   helper -> agent : {"type":"renew"}                  续约（唯一必需的）
 *                     {"type":"disarm","instance":"...","stop_id":"..."} / {"type":"arm"}
 *                       所有下行仍携带 token；instance 每脚本生成、跨重连稳定，stop_id 每次停止唯一。
 *                     {"type":"mode","clear":true|false}
 *                     {"type":"restore","on":true|false}
 *                     {"type":"targets","report":[...],"clear":[...]}
 *                       下发动态范围。report 必须是语义按键白名单的子集，clear
 *                       必须覆盖 report；空集合表示完全恢复原路径。
 */

/* ------------------------------------------------------------------ 常量 */

/* 脚本代次。**必须改了逻辑就改它**：助手把它与内嵌脚本里的值比对，不一致就打
   [AGENT-STALE]。没有它，"改了 agent 但宿主里跑的还是上一代"是完全静默的——
   握手正常、命令照发、日志漂亮，只有按键行为是旧的（2026-09-26 哨兵键那次
   就是这样白跑了一轮：以为在验新逻辑，其实接管的是旧实例）。 */
var AGENT_BUILD = '2026-10-03.cleanup-instance';

var TARGET_IOCTL = 0x80018483;
var TARGET_USAGES = [
  0x00F1, 0x0028, 0x0035, 0x004A, 0x004F, 0x0050, 0x0051,
  0x0052, 0x0065, 0x0066, 0x007F, 0x0080, 0x0081
];

/* 语音键热键合成（豆包支持，2026-09-28 白名单）。
 * from 只允许语音键 0x003E：ATVV 会话走 BLE 协议层不经过它，且它在
 * Windows 输入流本来就未映射——槽内替换零损失。
 * to 只允许已实测映射的合成 usage（USAGE_TO_VK，见探针 wudf_ioctl_synth.py）：
 * 0x00E6 实测产出 VK_RMENU(0xA5)、0x00E2 实测产出 VK_LMENU(0xA4)。
 * 未实测的 usage 一律拒绝——替换 usage 必须逐键实测。 */
var SYNTH_FROM_WHITELIST = [0x003E];
var SYNTH_TO_WHITELIST = [0x00E6, 0x00E2];
var TARGET_NAMES = {
  0x00F1: 'back', 0x0028: 'ok', 0x0035: 'tv', 0x004A: 'home',
  0x004F: 'right', 0x0050: 'left', 0x0051: 'down', 0x0052: 'up',
  0x0065: 'menu', 0x0066: 'power', 0x007F: 'volume_mute',
  0x0080: 'volume_up', 0x0081: 'volume_down'
};
var REPORT_ID = 0x01;

var LEASE_MS = 2000;          /* 超过此时间未收到续约就停止清键 */
var HB_MS = 500;              /* 所有权心跳周期；须显著短于 2s 租约 */
/* 按下帧呈现前的有界延迟（2026-10-03，见 Bugs/2026-10-03-first-press-lost-before-ime-switch.md）：
   应用在得知按下之后才切输入法（实测 ~53ms），而本替换在按下帧通过时立即生效——
   先于切换完成，目标输入法（豆包）收不到按下沿（第一按丢失）。应用在桥接握手声明
   能力（W 1）后由助手下发 gate 命令开启；初值 150ms（~2.8x 余量，真机数据后定稿）。
   释放帧不延迟；未开启或 sleep 不可用时行为不变（fail-open）。 */
var GATE_DELAY_MS = 150;
var CONNECT_TIMEOUT_MS = 3000;/* init() 最多阻塞宿主这么久 */
var RECONNECT_MS = 1000;
/*
 * 下行静默看门狗。**这是"重连"能不能真的工作所依赖的东西**。
 *
 * 为什么不靠读写报错：本机 frida 17.18.0 实测，对端关闭后继续 `output.write()`
 * **不抛错**（3/3），read 侧也没有可靠地给出"对端已关"的信号。也就是说
 * "助手退了/换了一个"这件事，在 socket 层**看不到**。
 * 但有一件事是确定的：助手每 500ms 发一次续约。**只要下行静默超过 RX_TIMEOUT_MS，
 * 就一定意味着对面不在了**——这条判据不依赖 frida 的错误语义。
 */
var RX_TIMEOUT_MS = 3000;
/* 连上了、hello 也发了，却一直没有收到续约 ⇒ 对面的令牌与我不同（我们被换代了）。
   这时继续每秒重连只会刷爆日志、并跟"应该被接管的那一个实例"抢连接。 */
var AUTH_TIMEOUT_MS = 4000;
var RECONNECT_MS_SLOW = 15000;
var MAX_CLEARS = 500000;
var MAX_LINE = 8192;

/* ------------------------------------------------------------------ 状态 */

var params = {};
var mode = 'clear';           /* clear | observe */
var restoreOnLeave = true;
var disarmed = false;

/* 语音键热键合成状态。槽内替换的电平语义：物理报告含 synthFrom → 槽位呈现
 * synthTo；物理释放 → 槽位自然回零。HID 报告是状态语义，usage 从报告中消失
 * 即 OS 收到该键的 UP——粘键在结构上不可能，无需补帧或状态跟踪。 */
var synthFrom = 0;
var synthTo = 0;
var gateDelayMs = 0;          /* 0 = 门未开启；>0 = 按下帧改写前先睡这么久 */
var hookListener = null;
var observedUsages = [];
var physicalUsages = [];
var blockedUsages = [];
var synthHeld = false;
var synthOffAckPending = false;
var synthHeldFrom = 0;
var synthHeldTo = 0;
var synthHeldHandle = '';
var synthGeneration = 0;
var synthWritesInFlight = 0;
var synthCompletionUncertain = false;
var stopPending = false;

var sock = null;
var connected = false;
var handshakeDone = false;
var rxBuffer = '';
var pumpRunning = false;

/* 出站队列状态（见下方「出站串行化」）。 */
var txQueue = [];
var txBusy = false;           /* 有一行在飞（等它的 Promise settle） */
var txSeq = 0;                /* 出站代次：只有最新一次的 settle 回调有权动状态 */
var txSince = 0;              /* 当前这行开始写的时刻（挂起兜底用） */

var lastRenewAt = 0;
var tConnect = 0;
var tLastRx = 0;              /* 最近一次收到下行数据的时刻（看门狗用） */
var retryNotBefore = 0;       /* 连接重试的最早时刻（鉴权失败后拉长退避） */
var connecting = false;       /* 有 connect 尚在飞行中（并发守卫，见 connectOnce） */
var attemptSeq = 0;           /* 尝试序号：只有最新一次尝试有权写 sock */
var tReady = Date.now();
/* 每次脚本加载生成，跨 socket 重连保持；只用于实例关联，鉴权仍由 token 承担。 */
var agentInstance = Process.id.toString(16) + '-' + tReady.toString(16);
for (var noncePart = 0; noncePart < 4; noncePart++) {
  agentInstance += '-' + ('00000000' + Math.floor(Math.random() * 0x100000000).toString(16)).slice(-8);
}
var stopRequestId = '';

var stat = {
  ioctl_calls: 0,
  target_hits: 0,
  /* 只含额外清空键（clearUsages 里不属于 reportUsages）的报告数。 */
  canary_hits: 0,
  clears_ok: 0,
  clears_fail: 0,
  restores_ok: 0,
  restore_skipped: 0,
  kernel_changed: 0,
  edges_sent: 0,
  write_fail: 0,              /* 写失败/被拒的行数（不含"未连接"的丢弃） */
  /* 出站串行化的观测面（2026-10-02）。修复前 `targets_ack`/`synth:frame` 整类丢失
     在计数里完全不可见：`write_fail` 不涨、没有日志、连接也没断。这四个字段让
     "上行有没有真的出去、一次回调连写了几行、有没有再撞上单飞互斥"从日志一眼可读。 */
  tx_lines: 0,                /* 真正写完的出站行数（Promise resolve 计数） */
  tx_pending_peak: 0,         /* 队列峰值深度：>1 = 一次回调里连写过多行 */
  tx_overflow: 0,             /* 队列满被丢弃的行数 */
  tx_busy_drop: 0,            /* 因 `stream has outstanding operation` 被拒的行数（修复前=静默丢失量） */
  tx_stall: 0,                /* 写挂起超过 TX_STALL_MS 而被强制放行的次数 */
  /* `send_dropped` 与 `cmd_rejected` 曾经共用一个 `discarded` 字段，
     导致真机日志里出现"discarded=11"却没人知道那是"没连接上时想上报的 11 条心跳"
     还是"11 条令牌不符的命令"——两者的安全含义完全不同：
     前者是正常的重连窗口，后者意味着有进程在冒充助手。现已拆开。 */
  send_dropped: 0,            /* 未连接时被丢掉的上行（心跳/边沿/日志） */
  cmd_rejected: 0,            /* 下行命令被拒（令牌不符或形状非法） */
  connect_raced: 0,           /* 竞争落败而主动关闭的多余连接（见 connectOnce） */
  targets_applied: 0,         /* 成功应用的 targets 命令数（清空范围被下发过几次） */
  targets_rejected: 0,        /* 被护栏拒掉的 targets 命令数（形状非法 / 试图改上报集合） */
  synth_applied: 0,           /* 成功应用的 synth 命令数（含 off） */
  synth_rejected: 0,          /* 被白名单拒掉的 synth 命令数 */
  synth_hits: 0,              /* 执行了语音键替换的帧数 */
  gate_delays: 0,             /* 按下帧执行了门内延迟的帧数（真的睡着过） */
  gate_fail: 0,               /* Thread.sleep 不可用/失败次数（fail-open，仅计数） */
  lease_expired: 0,
  rx_timeouts: 0,
  read_errors: 0,             /* 读失败导致的断线（此前这条路径不计数，见 pump） */
  auth_rejected: 0,
  errors: 0
};

var pressed = {};             /* 当前按下的目标 usage（绝对状态） */

/* 两个集合必须分开，否则"哨兵键"会污染按键证据：
   - reportUsages：**上报**集合。由主程序动态下发，且只能取语义按键白名单的子集。
   - clearUsages ：**清空**集合。产品路径覆盖 reportUsages；验收时可追加哨兵键。
   哨兵键存在的理由：RC003 的三键在 Windows 侧本来就零事件，所以"清掉"与"不清"
   在外部看不出差别 —— 清一个本来可用的键（如主页 0x4A）才能让清空是否生效变得可见。 */
var reportUsages = [];
var clearUsages = [];
var targetGeneration = 0;

/* ------------------------------------------------------- 编解码与传输 */

/* 纯 ASCII 编解码：协议里不出现非 ASCII，避免依赖 QuickJS 是否带 TextEncoder。 */
function encodeAscii(text) {
  var ab = new ArrayBuffer(text.length);
  var view = new Uint8Array(ab);
  for (var i = 0; i < text.length; i++) view[i] = text.charCodeAt(i) & 0x7F;
  return view;
}

function decodeAscii(buf) {
  var u8 = new Uint8Array(buf);
  var out = '';
  for (var i = 0; i < u8.length; i++) out += String.fromCharCode(u8[i]);
  return out;
}

/* --------------------------------------------------------- 出站串行化 */

/* Frida 17 实测（2026-10-02，`agent_selftest.py` 用例 F 钉住，勿按直觉改）：
   `Socket.output.write()` 返回 **Promise**，而且**同一时刻只允许一个在飞**。
   上一次没 settle 时的第二次 write **不抛错、不排队**，而是立刻以
   `Error: stream has outstanding operation` **reject** —— 零字节送达。
   火并忘（fire-and-forget）的写法看不到这个 reject（无人处理它），于是
   「一次回调里连写多行」的上行只剩第一行到得了助手：真机上 `targets_ack`、
   `targets:applied`、`synth_ack`、`synth:frame` 就是这样整类消失的
   （定时器驱动的 `hb` 每 tick 只写一行，所以一直正常——这正是最难查的那种不对称）。

   修法：**所有**上行进同一队列，由 Promise 的 settle 驱动逐行发出：
   - FIFO ⇒ `targets_ack` 仍在 targets 应用**之后**发出，语义不提前；
   - 每行都在上一行 settle 之后才写 ⇒ 结构上不可能再撞上"单飞互斥"；
   - settle 回调带代次（txSeq）⇒ 断线/重连/挂起兜底后的迟到回调不得改写新状态。 */
var TX_QUEUE_MAX = 512;       /* 积压上限（≈4 分钟的 hb）；满了丢最新那条并计数 */
var TX_STALL_MS = 2000;       /* 单行写挂起超过这么久就强制放行队列 */

/* "stream has outstanding operation" 只做分类、不留原始文本（隐私红线）。 */
function isBusyWriteError(e) {
  return /outstanding operation/i.test(String(e));
}

/* 清空积压（换连接 / 断线）：这些都是"没发出去的上行"，如实计入 send_dropped。 */
function txDropAll() {
  if (txQueue.length > 0) {
    stat.send_dropped += txQueue.length;
    txQueue.length = 0;
  }
}

/* 队列 → socket 的唯一驱动。串行：一次只写一行，settle 后再写下一行。 */
function txPump() {
  if (txBusy || txQueue.length === 0) return;
  if (!connected || sock === null) { txDropAll(); return; }

  var mine = sock;
  var myTx = ++txSeq;
  var line = txQueue.shift();

  txBusy = true;
  txSince = Date.now();

  var p;
  try {
    p = mine.output.write(encodeAscii(line));
  } catch (e) {
    /* write 同步抛出（实测路径基本不可达）：这条丢了，并清空积压避免原地打转。 */
    if (myTx === txSeq) txBusy = false;
    stat.write_fail++;
    txDropAll();
    return;
  }

  if (!p || typeof p.then !== 'function') {
    /* 没有 Promise 的环境/桩（agent_logic_test.mjs 的桩就是这种）：按"已写完"处理。 */
    if (myTx !== txSeq) return;
    txBusy = false;
    stat.tx_lines++;
    txPump();
    return;
  }

  p.then(function () {
    if (myTx !== txSeq) return;        /* 已被更新的一代取代（断线/重连/兜底放行） */
    txBusy = false;
    stat.tx_lines++;
    txPump();
  }, function (e) {
    if (myTx !== txSeq) return;
    txBusy = false;
    if (isBusyWriteError(e)) stat.tx_busy_drop++;   /* 修复前每一行都死在这里且不留痕 */
    else stat.write_fail++;
    txPump();
  });
}

/* 入队 + 立即驱动。队列满就丢**最新**那条（保住已经排在前面的 ack/边沿顺序）。 */
function txEnqueue(line) {
  if (txQueue.length >= TX_QUEUE_MAX) { stat.tx_overflow++; return; }
  txQueue.push(line);
  if (txQueue.length > stat.tx_pending_peak) stat.tx_pending_peak = txQueue.length;
  txPump();
}

/* 挂起兜底：Promise 迟迟不 settle 时不让整个上行通道永久停摆。
   （仓库规则：后台机制的"偶发迟到"按必然事件设计——迟到回调不得污染新状态，
   所以这里推进代次，让旧回调回来后什么都不做。） */
function txCheckStall() {
  if (!txBusy) return;
  if (Date.now() - txSince < TX_STALL_MS) return;
  stat.tx_stall++;
  txSeq++;
  txBusy = false;
  logLine('tx:stall');                  /* 走队列，与其它上行同样串行 */
}

/* 唯一的上行出口。失败只计数，绝不抛出（抛出去会打断 Interceptor 回调）。
   只入队、不直接写：真正写入由 txPump 串行驱动（见上）。 */
function sendLine(obj) {
  if (!connected || sock === null) { stat.send_dropped++; return false; }
  var line;
  try { line = JSON.stringify(obj) + '\n'; } catch (e) { stat.write_fail++; return false; }
  txEnqueue(line);
  return true;
}

function logLine(msg) {
  sendLine({ type: 'log', t: Date.now(), msg: String(msg).slice(0, 400) });
}

/* 语音键帧轨迹（2026-10-02 排障新增）。真机出现"按了没反应"时，
   现有计数只能看到"最终有没有替换成功"，看不到卡在哪一步；这里在每帧
   语音键经过时打点，秒级限流防刷屏。 */
var SYNTH_TRACE = { window: 0, count: 0, maxPerSec: 8, dropped: 0 };
function synthTrace(msg) {
  var now = Date.now();
  if (now - SYNTH_TRACE.window >= 1000) { SYNTH_TRACE.window = now; SYNTH_TRACE.count = 0; }
  if (SYNTH_TRACE.count >= SYNTH_TRACE.maxPerSec) { SYNTH_TRACE.dropped++; return; }
  SYNTH_TRACE.count++;
  logLine('synth:frame ' + msg + (SYNTH_TRACE.dropped ? ' dropped=' + SYNTH_TRACE.dropped : ''));
  SYNTH_TRACE.dropped = 0;
}

/* ------------------------------------------------------------ 租约状态 */

/* 清键许可：助手在 LEASE_MS 内续过约，且未被显式解除。 */
function leaseOk() {
  if (disarmed || !connected) return false;
  if (lastRenewAt === 0) return false;   /* 还没收到第一次续约 ⇒ 从未武装 */
  return (Date.now() - lastRenewAt) <= LEASE_MS;
}

/* ------------------------------------------------------------ 命令处理 */

/* 校验下行给的 usage 数组：元素是 1..0xFFFF 的整数、无重复；允许空集。非法返回 null。
   usage 0 被拒绝：它在报告里表示"该槽为空"，不是一个可以按下的键。 */
function usageArrayOf(v) {
  if (!Array.isArray(v)) return null;
  var out = [];
  for (var i = 0; i < v.length; i++) {
    var n = v[i];
    if (typeof n !== 'number' || !isFinite(n)) return null;
    n = Math.floor(n);
    if (n <= 0 || n > 0xFFFF) return null;
    if (out.indexOf(n) >= 0) return null;
    out.push(n);
  }
  return out;
}

function sameUsageSet(a, b) {
  if (a.length !== b.length) return false;
  for (var i = 0; i < a.length; i++) if (a.indexOf(b[i]) < 0) return false;
  return true;
}

/* usage 列表 → 日志/上报用的十六进制串。 */
function usagesHex(us) {
  var out = [];
  for (var i = 0; i < us.length; i++) {
    var h = us[i].toString(16);
    while (h.length < 4) h = '0' + h;
    out.push('0x' + h);
  }
  return out.join(',');
}

function handleCommand(line) {
  tLastRx = Date.now();
  var cmd = null;
  try { cmd = JSON.parse(line); } catch (e) {
    /* 2026-09-29 run7 观察：targets/synth 的 ack 四轮全缺而 renew 正常——
       必须区分「字节没到 / 到了但损坏 / 处理了但 ack 没回」。parse 失败不再静默。 */
    logLine('cmd:parse_failed len=' + line.length + ' head=' + line.slice(0, 32));
    return;
  }
  if (!cmd || typeof cmd.type !== 'string') {
    logLine('cmd:bad_shape head=' + String(line).slice(0, 32));
    return;
  }
  /* 每条非 renew 下行都留痕：renew 500ms 一条太多，其余命令频次极低。 */
  if (cmd.type !== 'renew') logLine('cmd:' + cmd.type);

  /* 令牌校验：助手监听在 loopback，本地任意进程都能连上来冒充助手。
     带令牌时不匹配的命令一律忽略（尤其不能让伪造的 renew 把 agent 武装起来）。 */
  if (params.token) {
    if (cmd.token !== params.token) { stat.cmd_rejected++; return; }
  }

  if (cmd.type === 'renew') {
    if (stopPending) return;
    if (lastRenewAt === 0) logLine('renew:first');           /* 首次续约 = 武装时刻 */
    lastRenewAt = Date.now();
    if (handshakeDone === false) handshakeDone = true;
    return;
  }
  if (cmd.type === 'arm') {
    if (stopPending) { logLine('arm:rejected_cleanup_pending'); return; }
    disarmed = !installHook(); return;
  }
  if (cmd.type === 'disarm' || cmd.type === 'bye') {
    if (cmd.instance !== agentInstance || typeof cmd.stop_id !== 'string'
        || !/^[0-9a-zA-Z-]{1,128}$/.test(cmd.stop_id)) {
      stat.cmd_rejected++; logLine('stop:rejected_identity'); return;
    }
    stopRequestId = cmd.stop_id;
    stopCapture(cmd.type); return;
  }
  if (cmd.type === 'mode') {
    mode = (cmd.clear === false) ? 'observe' : 'clear';
    logLine('mode:' + mode);
    return;
  }
  /* 下发动态捕获范围。两条护栏：report 只能来自语义按键白名单；clear 必须覆盖
     report，避免出现“已经上报但没有清掉原生边沿”的双触发。空集合是关闭增强捕获
     时的 fail-open 配置。clear 仍可额外带哨兵键供人工验收。 */
  if (cmd.type === 'targets') {
    var rep = usageArrayOf(cmd.report);
    var clr = usageArrayOf(cmd.clear);
    if (rep === null || clr === null) {
      stat.cmd_rejected++; stat.targets_rejected++; logLine('targets:rejected_bad_array'); return;
    }
    for (var wi = 0; wi < rep.length; wi++) {
      if (TARGET_USAGES.indexOf(rep[wi]) < 0) {
        stat.cmd_rejected++; stat.targets_rejected++; logLine('targets:rejected_outside_whitelist'); return;
      }
    }
    var covers = true;
    for (var ti = 0; ti < rep.length; ti++) if (clr.indexOf(rep[ti]) < 0) covers = false;
    if (!covers) {
      stat.cmd_rejected++; stat.targets_rejected++; logLine('targets:rejected_clear_lacks_report'); return;
    }
    var nextGeneration = Number(cmd.generation);
    if (!isFinite(nextGeneration) || nextGeneration < 0) {
      stat.cmd_rejected++; stat.targets_rejected++; logLine('targets:rejected_bad_generation'); return;
    }
    /* 先释放被移出目标集的按键，避免切换映射时留下旧 DOWN。 */
    var retained = [];
    var before = currentUsages();
    for (var ri = 0; ri < before.length; ri++) if (rep.indexOf(before[ri]) >= 0) retained.push(before[ri]);
    blockedUsages = blockedUsages.filter(function (u) { return rep.indexOf(u) >= 0; });
    for (var bi = 0; bi < rep.length; bi++) {
      if (reportUsages.indexOf(rep[bi]) < 0 && physicalUsages.indexOf(rep[bi]) >= 0
          && blockedUsages.indexOf(rep[bi]) < 0) blockedUsages.push(rep[bi]);
    }
    reportUsages = rep.slice();
    clearUsages = clr.slice();
    targetGeneration = Math.floor(nextGeneration);
    emitEdgesIfChanged(retained);
    stat.targets_applied++;
    sendLine({ type: 'targets_ack', t: Date.now(), generation: targetGeneration, usages: reportUsages });
    logLine('targets:applied generation=' + targetGeneration + ' report=' + usagesHex(reportUsages)
      + ' clear=' + usagesHex(clearUsages));
    return;
  }
  /* 语音键热键合成配置。护栏：from 必须是语音键、to 必须是已实测映射的
     合成 usage（见 SYNTH_TO_WHITELIST 注释）。off 关闭并立即复位注入状态。 */
  if (cmd.type === 'gate') {
    /* 门内延迟开关（2026-10-03）：应用声明能力（W 1）后由助手下发；幂等。
       delay_ms 可覆盖常量（测试/调参），非法值回落 GATE_DELAY_MS。 */
    var gateOn = cmd.on !== false;
    var want = Number(cmd.delay_ms);
    gateDelayMs = gateOn ? ((want > 0 && want <= 2000) ? want : GATE_DELAY_MS) : 0;
    logLine('gate:' + (gateDelayMs > 0 ? 'on delay_ms=' + gateDelayMs : 'off'));
    return;
  }
  if (cmd.type === 'synth') {
    if (cmd.off === true) {
      synthFrom = 0; synthTo = 0;
      stat.synth_applied++;
      synthOffAckPending = synthHeld || synthWritesInFlight > 0 || synthCompletionUncertain;
      if (!synthOffAckPending) sendLine({ type: 'synth_ack', t: Date.now(), off: true });
      else logLine('synth:off_awaiting_physical_release');
      logLine('synth:off');
      return;
    }
    var sFrom = Number(cmd.from), sTo = Number(cmd.to);
    if (SYNTH_FROM_WHITELIST.indexOf(sFrom) < 0 || SYNTH_TO_WHITELIST.indexOf(sTo) < 0
        || sFrom === sTo) {
      stat.synth_rejected++;
      logLine('synth:rejected from=' + usagesHex([sFrom]) + ' to=' + usagesHex([sTo]));
      return;
    }
    synthFrom = sFrom; synthTo = sTo;
    stat.synth_applied++;
    sendLine({ type: 'synth_ack', t: Date.now(), from: synthFrom, to: synthTo });
    logLine('synth:applied from=' + usagesHex([synthFrom]) + ' to=' + usagesHex([synthTo]));
    return;
  }
  if (cmd.type === 'restore') { restoreOnLeave = (cmd.on !== false); return; }
}

function stopCapture(reason) {
  disarmed = true;
  lastRenewAt = 0;
  gateDelayMs = 0;
  releaseEdges(reason);
  emitObservation([]);
  if (synthHeld || synthWritesInFlight > 0) {
    stopPending = true;
    logLine('stop:awaiting_physical_release');
    return;
  }
  completeStop();
}

function completeStop() {
  stopPending = false;
  synthFrom = 0; synthTo = 0;
  var detached = false;
  try {
    if (hookListener !== null) {
      hookListener.detach();
      hookListener = null;
      Interceptor.flush();
    }
    detached = true;
  } catch (e) { logLine('stop:detach_failed'); }
  sendLine({ type: 'stopped', instance: agentInstance, stop_id: stopRequestId,
    hook_detached: detached, released_all: true });
}

function releaseEdges(reason) {
  var usages = [];
  for (var u in pressed) if (pressed[u]) usages.push(parseInt(u, 10));
  if (usages.length === 0) return;
  pressed = {};
  sendLine({ type: 'edge', t: Date.now(), usages: [], released_all: true, reason: reason });
}

/* ------------------------------------------------------------ 读泵 */

function pump() {
  if (!connected || sock === null || pumpRunning) return;
  /* 记下"本次读属于哪条连接"。回调里必须核对，否则**旧连接的失败会误杀刚接上的
     新连接**（并发重连场景下这是真实可达的）。 */
  var mine = sock;
  pumpRunning = true;
  mine.input.read(4096).then(function (buf) {
    pumpRunning = false;
    if (!connected || sock !== mine) return;
    var len = 0;
    try { len = new Uint8Array(buf).length; } catch (e) { len = 0; }
    if (len === 0) { setTimeout(pump, 200); return; }
    rxBuffer += decodeAscii(buf);
    tLastRx = Date.now();                                 /* 看门狗：任何下行字节都算活着 */
    if (rxBuffer.length > MAX_LINE * 4) rxBuffer = '';   /* 异常保护 */
    var lines = rxBuffer.split('\n');
    rxBuffer = lines.pop();
    for (var i = 0; i < lines.length; i++) {
      if (lines[i].length > 0) handleCommand(lines[i]);
    }
    pump();
  }).catch(function () {
    pumpRunning = false;
    if (sock !== mine) return;     /* 已被更新的连接取代：别动它 */
    /* 读失败 = 这条连接死了。**必须走 dropConnection**：
       此前这里只写 `connected = false`，于是 retryNotBefore / sock / tLastRx 都不复位，
       而且"连接死过"这件事在统计里完全不可见。
       2026-09-23 自检台（用例 D）实测到后果：重连确实发生了，但 `rx_timeouts=0`、
       `auth_rejected=0`，任何计数器都读不出"连接断过一次"——排查时会被读成
       "看门狗从未触发"。 */
    stat.read_errors++;
    dropConnection('read_error', false);
  });
}

/* ------------------------------------------------------------ 连接 */

function connectOnce() {
  var port = params.port;
  if (!port) return Promise.reject(new Error('no port parameter'));

  /* 并发守卫：`Socket.connect` 到**没人监听**的端口要约 2.2s 才 reject
     （见文件头实测），而心跳周期是 1s —— 于是"启动新助手"的那一瞬间会同时有
     2~3 个 connect 在飞。全都成功之后，每个 `.then` 都写一次全局 `sock`：
     后写的赢，先写的那个 socket 变成无人引用的垃圾，被 JS GC 终结时发出 RST，
     在助手侧表现为 `10054/10053` + `[DISCONNECT] edges=0`。
     2026-09-23 真机接管轮实测：3 条连接、2 条被 RST，好在那一次"最新的一条"赢了；
     顺序反过来就会把好连接reset掉，接管路径变得看运气。

     修法：`attemptSeq` 决定谁有权写 `sock`（只有最新一次尝试），输家**主动 close**
     自己的 socket（主动关是 FIN，不会在助手侧变成 RST）。 */
  var myAttempt = ++attemptSeq;
  connecting = true;

  return Socket.connect({ family: 'ipv4', host: '127.0.0.1', port: port })
    .then(function (conn) {
      if (myAttempt === attemptSeq) connecting = false;
      if (connected || myAttempt !== attemptSeq) {
        stat.connect_raced++;
        try { if (typeof conn.close === 'function') conn.close(); } catch (e) { /* ignore */ }
        return false;
      }
      sock = conn;
      connected = true;
      rxBuffer = '';
      /* 新连接 = 出站的新一代：旧队列属于上一条连接（连不上的那些上行），
         作废并如实计数；代次推进让旧连接的迟到 settle 回调什么也不做。 */
      txDropAll();
      txSeq++;
      txBusy = false;
      txSince = 0;
      try { if (typeof conn.setNoDelay === 'function') conn.setNoDelay(true); } catch (e) { /* 非关键 */ }
      tConnect = Date.now();
      tLastRx = Date.now();
      sendLine({
        type: 'hello',
        t: tConnect,
        token: params.token || '',
        agent: 'rc003_agent/1',
        build: AGENT_BUILD,
        instance: agentInstance,
        pid: Process.id,
        arch: Process.arch,
        mode: mode,
        restore: restoreOnLeave,
        lease_ms: LEASE_MS,
        targets: reportUsages,
        target_generation: targetGeneration
      });
      pump();
      return true;
    }, function (err) {
      /* 失败的尝试也要归还 `connecting`（且只由最新那次尝试归还），
         否则一次 connect 超时就会把后面所有重连都挡在门外。 */
      if (myAttempt === attemptSeq) connecting = false;
      throw err;
    });
}

/* 断线后重连。**这条路径同时承担"助手重启但注入未重做"的情形**：
   DLL 已加载，LoadLibrary 不会重跑构造，agent 实例仍在，靠它连回新助手（令牌跨运行稳定）。

   但"断了"这件事在 socket 层看不到（写入不抛错，见文件头实测），所以断线判定放在
   heartbeat 里由**下行静默**给出，不在这里。 */
function ensureConnected() {
  if (connected) return;
  if (connecting) return;                       /* 已有一次在飞，别叠加（见 connectOnce） */
  if (Date.now() < retryNotBefore) return;      /* 鉴权失败后的慢速退避 */
  connectOnce().catch(function () { /* 下一轮再试 */ });
}

/* 把当前连接判死：关闭 socket、进入退避、等下一轮重连。
   `slow` = true 用于"连上了但对面不认我们"（鉴权失败），退避要长得多。 */
function dropConnection(reason, slow) {
  logLine('dropped:' + reason);          /* 必须在置 connected=false **之前**发 */
  connected = false;
  retryNotBefore = Date.now() + (slow ? RECONNECT_MS_SLOW : RECONNECT_MS);
  tConnect = 0;
  tLastRx = 0;
  pumpRunning = false;
  /* 出站队列属于这条已经判死的连接：推进代次、丢弃积压（迟到回调不得污染新连接）。
     `logLine('dropped:…')` 那一行已经在上面被发出（txPump 同步启动），不受这里影响。 */
  txSeq++;
  txBusy = false;
  txDropAll();
  try { if (sock !== null && typeof sock.close === 'function') sock.close(); } catch (e) { /* ignore */ }
  sock = null;
}

/* ------------------------------------------------------------ 报告处理 */

function targetSetIn(bytes) {
  var slots = [3, 5, 7];
  var found = [];
  for (var i = 0; i < slots.length; i++) {
    var o = slots[i];
    var u = bytes[o] | (bytes[o + 1] << 8);
    if (u === 0) continue;
    /* 用 reportUsages 而不是 clearUsages：额外哨兵键只清、不上报。 */
    if (reportUsages.indexOf(u) >= 0 && blockedUsages.indexOf(u) < 0 && found.indexOf(u) < 0) found.push(u);
  }
  return found;
}

/* 听键独立于映射目标：只报告白名单状态，不扩大清键/执行范围。 */
function knownUsagesIn(bytes) {
  var found = [];
  for (var o = 3; o <= 7; o += 2) {
    var usage = bytes[o] | (bytes[o + 1] << 8);
    if (TARGET_USAGES.indexOf(usage) >= 0 && found.indexOf(usage) < 0) found.push(usage);
  }
  return found.sort(function (a, b) { return a - b; });
}

function emitObservation(usages) {
  if (sameSet(observedUsages, usages)) return;
  observedUsages = usages.slice();
  sendLine({ type: 'observed', t: Date.now(), usages: observedUsages });
}

/* 清空集合视角。**与 targetSetIn 必须是两个函数**，这不是重复代码：
   2026-09-26 真机事故——canary（主页 0x004A）明明已下发（`targets:applied ... canary=1`），
   用户按主页键光标照常跳到行首。根因：onEnter 的早退门禁当时用的是 `targetSetIn()`
   （上报集合=三键），于是"只含哨兵键的报告"在门禁处就 return 了，下面那段用
   clearUsages 的清空循环**永远执行不到**——哨兵键从第一天起就是死代码。
   门禁必须问"这份报告有没有要清的东西"，而不是"有没有要上报的东西"。 */
function clearSetIn(bytes) {
  var slots = [3, 5, 7];
  var found = [];
  for (var i = 0; i < slots.length; i++) {
    var o = slots[i];
    var u = bytes[o] | (bytes[o + 1] << 8);
    if (u === 0) continue;
    if (clearUsages.indexOf(u) >= 0 && blockedUsages.indexOf(u) < 0 && found.indexOf(u) < 0) found.push(u);
  }
  return found;
}

/* 合成视角：这份报告里有没有语音键（等待被替换）。门禁必须三类合一
   （上报 ∪ 清空 ∪ 合成）——2026-09-26 canary 死代码事故的同款坑：
   只含语音键的报告若在门禁处早退，替换逻辑永远执行不到。 */
function synthSetIn(bytes) {
  return containsUsage(bytes, synthFrom);
}

function containsUsage(bytes, usage) {
  if (usage === 0) return false;
  var slots = [3, 5, 7];
  for (var i = 0; i < slots.length; i++) {
    var o = slots[i];
    var u = bytes[o] | (bytes[o + 1] << 8);
    if (u === usage) return true;
  }
  return false;
}

/* 唯一门禁。抽成函数是为了能被 `agent_logic_test.mjs` 直接调用（见同目录），
   把这次缺陷钉成回归项——纯静态的"源码里有没有这行"拦不住下一个人改回去。 */
function shouldTouchReport(bytes) {
  return targetSetIn(bytes).length > 0 || clearSetIn(bytes).length > 0
    || synthSetIn(bytes);
}

function sameSet(a, b) {
  if (a.length !== b.length) return false;
  for (var i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

function currentUsages() {
  var out = [];
  for (var u in pressed) if (pressed[u]) out.push(parseInt(u, 10));
  out.sort(function (x, y) { return x - y; });
  return out;
}

function emitEdgesIfChanged(found) {
  var sorted = found.slice().sort(function (x, y) { return x - y; });
  if (sameSet(sorted, currentUsages())) return;
  pressed = {};
  for (var i = 0; i < sorted.length; i++) pressed[sorted[i]] = true;
  stat.edges_sent++;
  sendLine({
    type: 'edge',
    t: Date.now(),
    usages: sorted,
    buttons: sorted.map(function (u) { return TARGET_NAMES[u] || ('0x' + u.toString(16)); }),
    n: sorted.length,
    /* 状态变化也有 reason：否则日志里会打出一条空的 `reason=`，
       而 `reason` 存在的意义就是区分"正常状态变化"与"被强制释放"。 */
    reason: 'state'
  });
}

function writeReport(ptr, bytes) {
  var buf = new Uint8Array(bytes.length);
  for (var i = 0; i < bytes.length; i++) buf[i] = bytes[i];
  try {
    ptr.writeByteArray(buf.buffer);
    return { ok: true, path: 'direct' };
  } catch (e1) {
    try {
      Memory.protect(ptr, bytes.length, 'rw-');
      ptr.writeByteArray(buf.buffer);
      return { ok: true, path: 'protect' };
    } catch (e2) {
      return { ok: false, path: 'none', err: String(e1) + ' / ' + String(e2) };
    }
  }
}

function installHook() {
  if (hookListener !== null) return true;
  var ntdll = Process.getModuleByName('ntdll.dll');
  var fn = ntdll.getExportByName('NtDeviceIoControlFile');
  if (fn === null || fn.isNull()) {
    logLine('hook:export_missing');
    return false;
  }

  hookListener = Interceptor.attach(fn, {
    onEnter: function (args) {
      this.hit = false;
      try {
        stat.ioctl_calls++;
        if (args[5].toUInt32() !== TARGET_IOCTL) return;

        var inLen = args[7].toUInt32();
        var outLen = args[9].toUInt32();
        if (inLen !== 8 || outLen < 9) return;

        var outPtr = args[8];
        if (outPtr === null || outPtr.isNull()) return;

        var inPtr = args[6];
        if (inPtr === null || inPtr.isNull()) return;
        var inBytes = new Uint8Array(inPtr.readByteArray(8));
        if (inBytes[4] !== 0x02 || inBytes[5] !== 0x01) return;

        if (outPtr.readU8() !== REPORT_ID) return;

        this.hit = true;
        this.outPtr = outPtr;
        this.outLen = outLen;
        this.cleared = false;
        this.restored = false;

        var bytes = new Uint8Array(outPtr.readByteArray(9));
        this.reportHandle = args[0].toString();
        this.ioStatus = args[4];
        this.releaseGeneration = synthHeld && !containsUsage(bytes, synthHeldFrom) ? synthGeneration : -1;
        physicalUsages = knownUsagesIn(bytes);
        blockedUsages = blockedUsages.filter(function (u) { return physicalUsages.indexOf(u) >= 0; });
        // 停止只放行原报告；必须在 onLeave 确认同 handle 的同步完成后才撤钩。
        if (stopPending) return;
        if (leaseOk()) emitObservation(physicalUsages);
        var found = targetSetIn(bytes);      /* 主程序动态上报集合 */
        var toClear = clearSetIn(bytes);     /* 上报集合 + 哨兵键（如有） */
        this.orig = bytes;

        /* 边沿上报必须早于/独立于清键：即使租约过期不上报也要如实反映状态 */
        emitEdgesIfChanged(found);

        /* 门禁走 `shouldTouchReport`（= 上报 ∪ 清空），**不能**只走 found：
           只含哨兵键的报告会被漏掉，哨兵键就永远清不掉（2026-09-26 真机）。 */
        if (!shouldTouchReport(bytes)) return;

        if (found.length > 0) stat.target_hits++;
        /* 哨兵键命中单独计数：它不上报边沿，若混进 target_hits，
           "target_hits × 2 == edges_sent"这条交叉校验就再也读不懂了。 */
        for (var ci = 0; ci < toClear.length; ci++) {
          if (reportUsages.indexOf(toClear[ci]) < 0) { stat.canary_hits++; break; }
        }

        var synthSeen = synthSetIn(bytes);
        if (synthSeen) {
          synthTrace('seen mode=' + mode + ' lease=' + (leaseOk() ? 1 : 0)
            + ' disarmed=' + (disarmed ? 1 : 0) + ' connected=' + (connected ? 1 : 0)
            + ' clears_ok=' + stat.clears_ok);
        }

        if (!leaseOk()) { stat.lease_expired++; if (synthSeen) synthTrace('skip reason=lease'); return; }
        if (mode !== 'clear') { if (synthSeen) synthTrace('skip reason=mode_' + mode); return; }
        if (stat.clears_ok >= MAX_CLEARS) { if (synthSeen) synthTrace('skip reason=clear_cap'); return; }

        /* 门内延迟（2026-10-03）：把语音键按下帧的呈现推迟到应用的输入法切换完成
           之后（见 GATE_DELAY_MS）。只作用于按下帧（synthSeen）；释放帧不经过这里
           ⇒ 呈现成对性不变。任何异常 fail-open（不延迟 = 现状），只计数 + 记日志。 */
        if (synthSeen && gateDelayMs > 0) {
          try {
            Thread.sleep(gateDelayMs);
            // 延迟期间配置/租约可能被撤销；迟到报告不得重新呈现合成按下。
            if (!leaseOk() || synthFrom === 0) return;
            stat.gate_delays++;
            logLine('synth:gate delay_ms=' + gateDelayMs);
          } catch (e) {
            stat.gate_fail++;
            logLine('synth:gate fail=' + String(e).slice(0, 80));
          }
        }

        /* 只清目标 usage 所在的两个字节，其余一字节不动。
           语音键合成（synthFrom !== 0 时）在同一循环里做槽内替换：
           物理报告含语音键 → 该槽呈现合成 usage；物理释放 → 报告自然回零，
           合成键随之消失（HID 状态语义 = UP），粘键在结构上不可能。 */
        var patched = bytes.slice();
        var changed = false;
        var synthChanged = false;
        for (var i = 0; i < 3; i++) {
          var o = 3 + i * 2;
          var u = bytes[o] | (bytes[o + 1] << 8);
          if (u === 0) continue;
          if (synthFrom !== 0 && u === synthFrom) {
            patched[o] = synthTo & 0xff;
            patched[o + 1] = (synthTo >> 8) & 0xff;
            changed = true;
            synthChanged = true;
            continue;
          }
          /* 用 clearUsages：产品路径覆盖动态目标；验收时可能多一个哨兵键。 */
          if (clearUsages.indexOf(u) < 0 || blockedUsages.indexOf(u) >= 0) continue;
          patched[o] = 0;
          patched[o + 1] = 0;
          changed = true;
        }
        if (!changed) { if (synthSeen) synthTrace('skip reason=nochange'); return; }

        var res = writeReport(outPtr, patched);
        if (!res.ok) { stat.clears_fail++; if (synthSeen) synthTrace('skip reason=write_fail'); return; }

        var back = new Uint8Array(outPtr.readByteArray(9));
        if (back[0] === patched[0] && back[3] === patched[3] &&
            back[4] === patched[4] && back[5] === patched[5] &&
            back[6] === patched[6] && back[7] === patched[7] &&
            back[8] === patched[8]) {
          stat.clears_ok++;
          if (synthChanged) stat.synth_hits++;
          if (synthChanged) {
            synthHeld = true;
            synthHeldFrom = synthFrom; synthHeldTo = synthTo;
            synthHeldHandle = this.reportHandle;
            synthGeneration++;
            synthWritesInFlight++;
            this.synthWritten = true;
          }
          if (synthChanged) synthTrace('replace ok to=0x' + synthTo.toString(16));
          this.cleared = true;
        } else {
          stat.clears_fail++;
          if (synthSeen) synthTrace('skip reason=verify_mismatch');
        }
      } catch (e) {
        stat.errors++;
      }
    },

    onLeave: function (retval) {
      if (this.hit && this.ioStatus) {
        var completed = false;
        try {
          completed = retval.toUInt32() === 0 && !this.ioStatus.isNull()
            && this.ioStatus.readU32() === 0
            && Number(this.ioStatus.add(Process.pointerSize).readU64().toString()) === 9;
        } catch (e) { /* 完成语义不可读，保持未确认。 */ }
        if (this.synthWritten) {
          synthWritesInFlight--;
          // STATUS_PENDING 无完成通知可用，不保存裸指针，也不假报已释放。
          if (retval.toUInt32() === 0x103) {
            synthCompletionUncertain = true;
            logLine('synth:completion_unconfirmed reason=pending');
          }
        }
        if (completed && this.releaseGeneration === synthGeneration
            && this.reportHandle === synthHeldHandle && synthWritesInFlight === 0
            && !synthCompletionUncertain) {
          synthHeld = false;
          synthHeldFrom = 0; synthHeldTo = 0; synthHeldHandle = '';
          if (synthOffAckPending) {
            synthOffAckPending = false;
            sendLine({ type: 'synth_ack', t: Date.now(), off: true });
          }
          if (stopPending) completeStop();
        } else if (stopPending && this.releaseGeneration >= 0 && !completed) {
          logLine('stop:release_unconfirmed reason=ioctl_not_completed');
        }
      }
      if (!this.hit || !this.cleared || !restoreOnLeave) return;
      try {
        var cur = new Uint8Array(this.outPtr.readByteArray(9));
        /* 内核若在调用期间改写过缓冲区，就放弃回写并如实计数 */
        if (cur[0] !== this.orig[0] || cur[3] !== 0 || cur[4] !== 0 ||
            cur[5] !== 0 || cur[6] !== 0 || cur[7] !== 0 || cur[8] !== 0) {
          stat.kernel_changed++;
          return;
        }
        var res = writeReport(this.outPtr, this.orig);
        if (!res.ok) { stat.restore_skipped++; return; }
        stat.restores_ok++;
      } catch (e) {
        stat.errors++;
      }
    }
  });

  return true;
}

/* ------------------------------------------------------------ 生命周期 */

function heartbeat() {
  var now = Date.now();

  /* 连接健康判定（顺序有意义：先判"对面不认我们"，再判"对面不在了"）。 */
  if (!stopPending && connected && tConnect !== 0 && lastRenewAt === 0 && (now - tConnect) > AUTH_TIMEOUT_MS) {
    stat.auth_rejected++;
    dropConnection('auth_mismatch', true);
  } else if (!stopPending && connected && tLastRx !== 0 && (now - tLastRx) > RX_TIMEOUT_MS) {
    /* 下行静默超过看门狗窗口 ⇒ 助手不在了（或被换代）。这是唯一可靠的断线判据。 */
    stat.rx_timeouts++;
    dropConnection('rx_silence', false);
  }

  ensureConnected();
  /* 出站兜底：队列里若有积压（例如上一次 txPump 因未连接而早退），每个心跳周期推一次；
     同时检查是否有单行写挂起（迟到回调按必然事件处理）。 */
  txCheckStall();
  txPump();
  var leaseNow = leaseOk();
  if (!leaseNow && lastRenewAt !== 0 && connected && !disarmed) {
    /* 租约刚过期：停止清键，并释放可能仍被记为按下的状态 */
    releaseEdges('lease_expired');
    lastRenewAt = 0;
  }
  sendLine({
    type: 'hb',
    t: Date.now(),
    up: Math.round((Date.now() - tReady) / 1000),
    connected: connected,
    handshake: handshakeDone,
    mode: mode,
    /* 上报两个集合：验收时一眼能看出"清空范围到底是不是我以为的那个"。
       此前这只能靠助手侧推断，agent 实际用的是什么都没人知道。 */
    report_usages: usagesHex(reportUsages),
    clear_usages: usagesHex(clearUsages),
    synth_from: synthFrom,
    synth_to: synthTo,
    gate_delay_ms: gateDelayMs,
    target_generation: targetGeneration,
    restore: restoreOnLeave,
    disarmed: disarmed,
    lease_ok: leaseOk(),
    since_renew_ms: lastRenewAt === 0 ? -1 : (Date.now() - lastRenewAt),
    stat: {
      ioctl_calls: stat.ioctl_calls,
      target_hits: stat.target_hits,
      canary_hits: stat.canary_hits,
      clears_ok: stat.clears_ok,
      clears_fail: stat.clears_fail,
      restores_ok: stat.restores_ok,
      kernel_changed: stat.kernel_changed,
      edges_sent: stat.edges_sent,
      write_fail: stat.write_fail,
      /* 出站串行化的观测面：tx_lines 不涨 = 上行根本没出去；
         tx_busy_drop>0 = 又撞上了 write 单飞互斥（修复回归）。 */
      tx_lines: stat.tx_lines,
      tx_pending_peak: stat.tx_pending_peak,
      tx_overflow: stat.tx_overflow,
      tx_busy_drop: stat.tx_busy_drop,
      tx_stall: stat.tx_stall,
      send_dropped: stat.send_dropped,
      cmd_rejected: stat.cmd_rejected,
      connect_raced: stat.connect_raced,
      targets_applied: stat.targets_applied,
      targets_rejected: stat.targets_rejected,
      /* synth 三兄弟此前漏在 hb 上报里（stat 对象在计数、日志永远看不到）——
         2026-09-29 run8 排查时踩到：clears_ok 在涨却无法确认是 synth 在干活。 */
      synth_applied: stat.synth_applied,
      synth_rejected: stat.synth_rejected,
      synth_hits: stat.synth_hits,
      gate_delays: stat.gate_delays,
      gate_fail: stat.gate_fail,
      read_errors: stat.read_errors,
      lease_expired: stat.lease_expired,
      rx_timeouts: stat.rx_timeouts,
      auth_rejected: stat.auth_rejected,
      errors: stat.errors
    }
  });
}

rpc.exports = {
  /* Gadget 会等这个 Promise settle 之后才放行宿主 entrypoint。
     绝不能让它无限等待：助手不在时 Socket.connect 永不 settle（实测），
     那样会把 HID 宿主永久挂住。所以用超时兜底，连接改为后台继续。 */
  init: function (stage, parameters) {
    params = parameters || {};
    if (params.leaseMs) LEASE_MS = params.leaseMs;
    if (params.mode) mode = params.mode;
    if (params.restore === false) restoreOnLeave = params.restore;

  logLine('agent:load build=' + AGENT_BUILD);
    var hookOk = installHook();

    var connectAttempt = connectOnce().then(function () { return 'connected'; })
      .catch(function (e) { return 'connect_failed:' + String(e).slice(0, 80); });

    var hostGate = new Promise(function (resolve) {
      setTimeout(function () { resolve('timeout'); }, CONNECT_TIMEOUT_MS);
    });

    setInterval(heartbeat, HB_MS);

    return Promise.race([connectAttempt, hostGate]).then(function (why) {
      return { stage: stage, hook: hookOk, gate: why, pid: Process.id };
    });
  },

  /* 助手主动卸载 / Gadget 被卸载时调用：立刻停止清键。 */
  dispose: function () {
    stopCapture('dispose');
    sendLine({ type: 'bye', t: Date.now(), stat: stat });
    connected = false;
    /* 卸载是"最后一次说话"：bye 已在上面交给 txPump 启动（同步发出），
       其余积压随连接作废，如实计入 send_dropped。 */
    txSeq++;
    txBusy = false;
    txDropAll();
    try { if (sock !== null && typeof sock.close === 'function') sock.close(); } catch (e) { /* ignore */ }
    sock = null;
    return true;
  }
};
