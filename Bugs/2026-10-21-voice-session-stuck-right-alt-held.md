# 2026-10-21 语音会话卡死 + 右 Alt 粘住：STOP 丢失后没有任何收尾兜底

- 发现日期：2026-10-21
- 状态：应用侧已修复（自动化 passed；RC003 真机正常语音与出字 passed；
  精确「STOP 丢失且无音频」路径仍 deferred）；助手 stale-tap 自动恢复未实施
- 影响范围：Windows 10/11；RC003 + 豆包输入法 + 「按住说话快捷键 = 右 Alt」；
  RC001 未涉及
- 功能点：ATVV 语音会话生命周期、按住说话快捷键注入、RC003 增强捕获助手
- 现象：按住遥控器语音键后，界面一直显示「正在接收语音」、按键页语音键一直
  显示按下；随后物理键盘像被 AltGr 按住一样不可用（字母键变成组合键）。
  等待十几分钟也不恢复；重启应用后界面恢复。
- 复现条件（本次现场）：增强捕获助手处于 stale-tap 失败态（`agent_never_acked`），
  报告层合成不可用，应用回落到 `SendInput` 注入右 Alt；随后 ATVV 的松手通知
  （控制帧 `00`）没有到达主程序。
- 正常预期：无论 STOP 是否丢失，松手后都必须结束会话、释放按住说话快捷键，
  不允许 UI 和修饰键永久停在按下状态。

## 证据（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，UTC）

现场时间轴（本地 +08，括号内为 UTC）：

- `20:03:10.444`（`12:03:10.444Z`）收到唯一的控制帧
  `direction=C preview=[04 03 02 75]`（ATVV 流开始）。
- `20:03:10.467` `voice_attempt session=117 tool=doubao path=send_input
  synth=0x00E6 synth_confirmed=false`；紧接着
  `chord_press result=ok session=117 gap_ms=80`——右 Alt 按下。
- `20:03:10.472` `audio_session generation=1 action=begin ... passed`。
- `20:03:11.273` / `20:03:12.973` `voice_verify ... mic=not_observed`。
- 此后到 `20:21` 只有 `direction=T preview=[0E 75]`（MIC_EXTEND，每 2.5 s），
  **没有任何新的 `direction=C` 控制帧**，也没有 `audio_session ... action=finish`。
- **没有任何 `chord_release` 行**；全日志最后一条 `chord_release` 在
  `05:08:34Z`（另一个会话）。
- `20:09:10` 的 `app_launcher ... physical_alt_held=false` 表明右 Alt 的 OS
  键态此时已不在按下——应用自己从未释放它；最可能是报告层旧 tap 仍把遥控器
  F5 翻译成右 Alt，物理松手时补了一个右 Alt UP。该解释是推断，未直接取证。

## 根因

### 已确认

1. **应用没有「STOP 丢失」的兜底。** 会话状态机只在收到 ATVV `00` 控制帧时
   进入 `Draining` 并释放按住说话快捷键。现场只收到 `04` 开始帧，之后控制
   通道零通知；GATT 连接状态没有断开，`ConnectionChanged` 也没有触发，
   所以现有的断连清理路径不会执行。
2. **按住说话快捷键是裸修饰键（右 Alt）。** `SendInput` 按下后，松手路径
   完全依赖上面的会话结束；结束不发生，右 Alt 就一直是逻辑按下状态。
   用户看到的「键盘不可用」与此一致：普通字母被当成 AltGr 组合。
3. **PR #279 与本次无关。** #279 修的是「切到微信后 `synth_confirmed` 把
   门禁重新举起、注入路径被跳过」，只改 `rc003_bridge.rs`；本现场走的是
   `path=send_input synth_confirmed=false`，不是 #279 的分支。本机安装包的
   `source_revision=945dd6a…` 不在当前仓库/远端，无法证明该包是否包含 #279，
   但即使包含也不会改变本次症状。

### 助手侧同时存在的故障（独立问题）

- 宿主 `WUDFHost.exe pid=37356` 里残留旧世代 tap；`11:54:46Z` 旧 agent 还能
  鉴权，之后不再重连。`11:55:58Z` 起每个新助手都得到
  `[ATTACH] verify=failed reason=no_authenticated_agent action=inject_new_generation
  skip_host_restart=shared_host`，然后 `agent_never_acked`，约 38 秒后退出，
  应用再拉起下一轮。
- 宿主是共享宿主（`members=3 rc003_members=1`），自动自愈只允许结束
  **独占 RC003** 的宿主；共享宿主分支不敢结束宿主的判据是正确的（同宿主还
  承载鼠标等设备），但「同宿主再注一代」这条实验路径在本机无效：
  `[VERIFY-MODULE] gadget_modules_in_host=2` 保持不变，`[REAP] kept_in_use=1`，
  说明注入没有产生可鉴权的新实例。
- 该故障使报告层合成不可用，应用只能回落到 `SendInput`；它是否直接导致
  ATVV STOP 丢失，现有证据不足，记为 `deferred`。但它让这次故障从「语音
  不可用」升级为「修饰键粘住 + 键盘不可用」。

## 修复（应用侧，最小范围）

文件：`crates/sayall-windows/src/ble.rs`

1. 新增 `VOICE_AUDIO_STALL_TIMEOUT = 8s` 与 `last_voice_audio_at` 跟踪。
   ATVV 按住期间持续推流，即使静音也有编码帧；连续 8 秒一帧音频都没有，
   判定 STOP 丢失或链路半死，调用 `abort_voice_session`：
   - 释放按住说话快捷键（右 Alt UP）；
   - 发送 `MIC_CLOSE`；
   - 中断音频并回到 `Idle`；
   - 落结构化日志
     `voice_session action=abort ... reason=audio_stall ... note=stop_not_received`。
2. 收到新的 `04` 开始帧而上一轮仍是 `Streaming` 时，先按
   `reason=stream_replaced` 收尾旧会话，再接受新会话。否则 pipeline 会以
   `AlreadyActive` 拒绝新开始，用户看到「语音键彻底没反应」。
3. 纯判定 `voice_stream_stalled` / `is_stream_start` 与单测，保证
   Idle/Draining 不误伤、满窗口才触发、一帧音频都没到也触发。

## 验证

- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows --lib`：**368 passed / 0 failed / 11 ignored**；
  新增 `voice_stall_requires_streaming_and_a_full_silent_window`、
  `stream_start_is_recognized_by_atvv_opcode` 均 passed。
- `cargo check --workspace`：passed。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed。
- 真机复验：**deferred**。需要重新构建/安装后制造一次 STOP 丢失（或临时
  阻断控制通知）并观察 8 秒内出现 `reason=audio_stall`、`chord_release`、
  UI 回到就绪、右 Alt 键态恢复。当前运行中的 20:03 会话仍属于旧构建。

## 二次现场：安装后卡在「正在连接遥控器」（同日 20:44–20:53）

安装新包后，旧实例的 `ble_session_shutdown` 超时（半死链路），新实例启动后：

- 日志只有 `device_from_address` / `device_properties` 完成；
- **没有 `service_discovery` 阶段**，工作线程在 `read_remote_model()` 里永久等待；
- `read_remote_model` 用的是 `BluetoothCacheMode::Uncached` 的 GATT 查询；
  Device Information 型号只是界面显示，却把整条连接路径卡死；
- 遥控器 PnP 节点全部 `OK`，不是设备离线；旧 `WUDFHost.exe pid=37356` 仍在，
  同时承载旧 tap。

处置与验证：

1. 对 RC003 做**只针对该设备节点**的
   `pnputil /remove-device` + `/scan-devices`（未结束共享宿主、未动蓝牙适配器）。
   `remove-device` 约 65 s 返回成功；随后新 `WUDFHost.exe pid=37164` 拉起。
2. 启动应用后 1.3 s 内完成全部连接阶段：
   `service_discovery 76ms` → `ble_identity action=ready`；
   助手 `gadget_modules_in_host=1`、`[HELLO] auth=true`、
   `voice_synth_ack confirm=agent`、`ownership_resumed`。
3. 蓝牙鼠标节点保持 `OK`，没有被一起移除。

根因修正：本次「连不上」不是遥控器没唤醒，而是旧 GATT 会话半死 +
`read_remote_model` 的无界 Uncached 查询把工作线程永久占住。已修：

- 新增 `wait_async`（`crates/sayall-windows/src/ble.rs`）：所有连接关键路径的
  WinRT 异步调用统一有界（设备创建 10 s，GATT 5 s）；超时 `Cancel` 并返回
  `OperationTimedOut`，让上层释放会话、进入重连。
- `read_remote_model` 改为 `BluetoothCacheMode::Cached`（型号是静态数据），
  缓存缺失就回退 Unknown，不再阻塞语音主路径。
- 覆盖 `find_service` / `find_characteristic` / CCCD 订阅 / 控制写入 /
  电量读取 / 型号读取。

## 真实语音复验（2026-10-21 晚）

- session 124（正常按住/松开）：`path=report_layer synth_confirmed=true`、
  `voice_effect result=replaced`、`audio_session begin → finish`，
  `submitted_samples=35520`；**没有 `audio_stall`**——正常会话不误触发。
- session 125（受控静默）：按住期间把应用暂停 10 s 再恢复。结果不是
  `audio_stall`，而是音频队列先报
  `audio_session ... error_code=queue_overflow reason=wasapi_pipeline_failed`，
  随后 `interrupt` 收尾；没有残留按住状态。**8 秒 `audio_stall` 路径本次
  未在真机触发（failed），但同场景的安全收尾 passed**。
- 受控静默无法精确模拟「STOP 丢失且无音频」；该路径仍以单元测试
  `voice_stall_requires_streaming_and_a_full_silent_window` 为准，真机
  精确复现标记 `deferred`。

## 本地测试包验证（2026-10-21 晚）

**Build 1（语音看门狗 + 重复开始自愈）**

- 产物：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
  - SHA-256 `B5767EEB35212CD8852C375E9D73558EE005E3DC01917B4F23396DCBA2E8C966`
  - 大小 `12,635,982` 字节
- 嵌入校验 `verify-frontend-embedded.ps1` 返回 0；静默安装 passed；app/helper/gadget
  哈希核对通过（app 与构建产物仅差 3 字节 bundle 标记）。
- 真实语音：session 124 正常按住/松开，`report_layer` + `audio_session finish`，
  无 `audio_stall`（正常会话不误触发）；session 125 受控静默见上。
- 助手 stale-tap 在该构建上仍复现；靠设备节点重建清除。

**Build 2（追加 WinRT 异步超时 + 型号缓存查询；当前已安装）**

- SHA-256 `948A5A4CF35C09E9882CD81BB0BE55C5209F8EF4BA9E2A841E953E288130AC85`
- 大小 `12,639,151` 字节
- 嵌入校验返回 0；安装器优雅退出 passed（`ble_session_shutdown` 160 ms，
  链路健康时正常收尾）；安装后的 exe 含新日志标记 `async_wait label=`。
- 启动后 1.1 s 内完成：
  `device_from_address 47 ms` → `service_discovery 407 ms` →
  `ble_identity action=ready`；助手 `voice_synth_ack confirm=agent`、
  `voice_synth_gate active=true`、`ownership_resumed`。
- 真实遥控器语音复验（Build 2）：**passed**——用户确认出字。日志：
  - session 128（`13:05:54Z`）：`voice_frame state=replaced`、
    `voice_effect result=replaced`、`audio_session begin → finish`
    `submitted_samples=27120`；无 `audio_stall`。
  - session 1（`13:10:25Z`）：同型，`submitted_samples=47280`；无
    `audio_stall`、无残留按住。
  - `voice_verify mic=not_observed` 在豆包上长期存在（历史正常会话同样如此），
    不是本次故障判据；用户可见的文字输出是最终判据。

## 边界

- 本次只修「会话卡死/修饰键粘住」，不修助手 stale-tap 本身；共享宿主下的
  旧 tap 仍会让报告层合成不可用，并继续触发 `capture_inactive` 自愈循环。
  该问题的安全修法是「只重启 RC003 设备节点、不结束共享宿主」，需要真机
  实验后单独提交。
- 8 秒是工程取值，不是真机标定值；若冷启动首帧可能更慢，需要真机日志
  再调整（当前正常会话首帧在 1 秒内）。
- 隐私检查：未包含蓝牙地址、HID 路径、用户名、语音内容或凭据；仅使用
  `%LOCALAPPDATA%` 与环境变量形式。
