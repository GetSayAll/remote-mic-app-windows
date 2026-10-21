# RC003 增强捕获 renew 停顿被判成鉴权失败，单次恢复要等 15 秒

- 发现日期：2026-10-21（现场日志 `03:49–04:19Z`，本地 11:49–12:19）
- 状态：自动化通过，等待真机验证
- 影响范围：SayAll Windows 1.0.0 preview `3376437`、Windows 11、RC003、
  「全按键支持」开启；RC001 同链路未测
- 功能点：RC003 增强捕获（helper ↔ 宿主内 agent 的 renew/hb 链路、应用侧所有权看门狗）

## 现象

「支持更多输入工具 / 全按键支持」已开启，但界面反复显示“还没有生效，正在自动重试”；
应用自动重启助手后短时间恢复，几分钟后再次失效。遥控器 BLE 没有断（当天只在 11:11
连接一次），问题不在蓝牙侧。

## 证据

同一份诊断日志（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`）：

- `03:49:58–04:19:40Z`：`ownership_timeout` 33 次；
- `03:53:19 / 03:57:16 / 04:07:15 / 04:17:37Z`：`capture_recover ... attempt=1`，
  4 次自动重启助手；前三次约 2s 恢复，最后一次约 20s；
- 助手侧 `[RENEW] state=write_failed` 4 次；
- `dropped:auth_mismatch` 5 次；每次之后 `[DISCONNECT]`，约 15–18s 后才重新
  `[ACCEPT]/[HELLO]`；
- `[WARN] 超过 5 秒没有 agent 心跳` 15 条；
- 关键形状：`04:17:11Z` 新连接已 `renew:first`，`04:17:29Z` 仍判
  `auth_mismatch`，`alive_s=18`。说明这不是“从未收到续约”，而是“收到过续约、
  之后停顿”。
- 同一日志 `04:52:39–04:52:58Z` 又出现一轮更完整的现场：
  - `04:52:39.642` 助手 `[HB] lease_ok=false renew_age_ms=0`（agent 侧租约已过期）；
  - `04:52:40.063` agent `dropped:auth_mismatch`；
  - `04:52:40.175` 助手续约线程仍然 `[RENEW] tick=4020 state=ok`（写没失败）；
  - `04:52:40.676` 才出现 `[RENEW] state=write_failed`（agent 关 socket 的结果）；
  - `04:52:56.592` 才重新 `[ACCEPT]`，`04:52:58.840` `renew:first`，空窗约 19s。
  这一轮说明**停顿首先发生在 agent 的读泵/JS 线程**，不是助手写阻塞；助手写超时
  是防御性加固，仍需探针确认 agent 侧被什么卡住。

## 根因

### 1. 已确认：鉴权判据把“租约过期”当成“从未续约”

`hardware/RC003/helper/agent/rc003_agent.js` 旧实现：

```js
if (connected && tConnect !== 0 && lastRenewAt === 0
    && (now - tConnect) > AUTH_TIMEOUT_MS) {
  stat.auth_rejected++;
  dropConnection('auth_mismatch', true);
}
```

`heartbeat()` 在租约过期时会执行 `lastRenewAt = 0`；于是长连接上任何一次
>2s 的续约停顿，4s 后都会被判成 `auth_mismatch`，走 `slow` 退避（固定 15s）。
真正的 `rx_silence` 路径（1s）因此永远轮不到。日志里“收到过续约但 18s 后仍
`auth_mismatch`”就是这条判据。

### 2. 防御性加固：助手写阻塞时不应同时停止读

`hardware/RC003/helper/src/main.rs` 的 `serve_connection` 是单线程
“读→写→读”循环，且 `TcpStream` 没有写超时。对端一时不读时，`targets` /
`synth` / `gate` 的 `write_all` 可能阻塞；阻塞期间它不再读 agent 的 `hb`，
agent 的续约也进不来，形成双向停顿。

**但 04:52 这一轮显示停顿先发生在 agent 侧**：助手 `[RENEW]` 仍在 `state=ok`，
agent 已 `lease_ok=false`。因此本次把写超时作为防御性加固，不把它写成已确认
的唯一根因；agent 侧被什么卡住（JS 线程、HID 钩子、Frida socket）由本次新增
的 `hb_gap_ms` / `pump_age_ms` / `stat.stalls` 探针在真机复现时定死。

## 修复

- `rc003_agent.js`
  - 新增 `everRenewed`（本次连接是否收到过有效 renew），与 `lastRenewAt`
    分离；`auth_mismatch` 只在 `!everRenewed` 时成立，租约过期后的停顿走
    `rx_silence`（1s）。
  - 鉴权失败退避从固定 15s 改为 2/4/8/15s 渐进；`rx_silence` / `read_error`
    仍为 1s，并清空鉴权失败计数。
  - 新连接重置 `everRenewed`；`renew` 置 true、清空 `authFailures`。
  - 探针：`hb` 增加 `hb_gap_ms`、`pump_age_ms`、`ever_renewed`、
    `stat.stalls`；超过 1.5s 时落一条 `stall` 事件（5s 限流）。
- `hardware/RC003/helper/src/main.rs`
  - agent socket 设 500ms 写超时；所有协议行经 `write_socket_line`，
    失败时落 `[WRITE-FAIL] action=… kind=write_timeout|peer_closed|io_error`
    并立刻结束本条 `serve_connection`，不再阻塞读循环。
  - **clone 也要单独设写超时**：Windows 的 `TcpStream::try_clone` 走
    `WSADuplicateSocket`，`SO_SNDTIMEO` 不随克隆句柄共享。自检实测：只在原
    stream 上设超时，clone（续约线程用的那条）写入仍会无限阻塞。现在 clone
    在存入 `shared` 前单独设 500ms。
  - accept 新连接时，在持有 `shared` 锁的同时清掉上一代遗留的 `conn_stale`，
    避免新连接刚被 accept 就被旧标志立刻打回（04:52:56 现场多绕了一次 accept）。
  - `[RENEW]` 失败带 `kind` / `elapsed_ms`；单次续约写 ≥100ms 落
    `state=slow`。
  - `[HB]` 增加 `hb_gap_ms` / `pump_age_ms` / `ever_renewed`（均来自 agent 的
    当前 hb），用于区分 agent 停顿与链路停顿；不复用 `session.last_rx` 计算
    “上一次 hb 年龄”（该值在 `handle_line` 前已被更新，恒为 0，会误导）。

预期效果：以 `04:52` 那轮为例，agent 收到过 renew 后停顿，旧路径是
`4s auth_mismatch + 15s 退避 + 约 1s 握手 ≈ 20s`；新路径走
`rx_silence（3s）+ 1s 重连`，应在 **4s 左右**恢复，低于应用侧 8s 的
`capture_recover` 阈值，不再触发助手重启。真机以实际日志为准。

## 验证

- `node hardware/RC003/helper/agent/agent_logic_test.mjs`：**63/63 passed**。
  - 阳性对照：把 `!everRenewed` 退回 `lastRenewAt === 0` 后 **60/63**，
    3 条红在目标断言（`rx=0 auth=1`、退避 2000ms 而非 1000ms、首次鉴权
    退避 4000ms 而非 2000ms）。
- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：**72 passed /
  0 failed**。
- `cargo run --manifest-path hardware/RC003/helper/Cargo.toml -- --selftest`：
  全部通过；新增“对端不读时 `write_socket_line` 在 500ms 档内失败”，实测
  ~500ms、`kind=write_timeout`。该用例**写入走 try_clone 后的句柄**，与续约
  线程同形；只在原 stream 设超时时它会失败（已实测），因此同时钉住了 clone
  必须单独设超时这一点。
- `cargo fmt --manifest-path hardware/RC003/helper/Cargo.toml -- --check`：passed。
- `cargo fmt --all -- --check`（主 workspace）：passed。
- `scripts/ci-preflight.ps1`：**7/7 PASS**（前端 450 passed | 10 skipped；workspace
  411 passed / 11 ignored；app 146 passed / 1 ignored；runtime simulation 编译检查通过）。
- `scripts/verify-rc003-helper.ps1 -Full`：**passed**。首跑有一次瞬时
  `sayall-windows` 用例失败（输出被截断，未捕获用例名）；随即
  `cargo test -p sayall-windows` 全绿，整脚本重跑也 passed，按瞬时抖动处理并在此留痕。
- 本地测试包：由 `scripts/build-local-test-package.ps1` 生成；release 助手与
  `src-tauri/sayall-helper.exe` 的 SHA-256 一致，确认包内是本次构建的助手。
  具体安装包路径与 SHA-256 放在交付消息里（不写进仓库，避免提交后重建导致
  元数据失效）。**未安装**（会打断当前正在运行的 preview 实例），安装/启动
  验证与实际按键效果归入真机 deferred。
- 真机 RC003 复现/验收：**deferred**。需要：
  1. 强杀助手后重连；2. 宿主重建；3. 睡眠唤醒；4. 冷态首用、连续按、快速连按。
  判据：恢复窗口 ≤3s、不再触发 8s `capture_recover`、没有粘键/丢键；
  `[RENEW] state=write_failed` 后不再出现 15s 的 `auth_mismatch` 空窗。
  RC001 同矩阵未测。

## 隐私检查

未包含用户名、主机名、蓝牙地址、HID 路径、端点 GUID、语音内容或凭据。
