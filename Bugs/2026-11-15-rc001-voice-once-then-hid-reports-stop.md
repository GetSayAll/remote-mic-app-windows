# RC001 首次语音后 HID 报告与 ATVV 停止帧同时消失，重连一直 Unreachable

- 发现日期：2026-11-15
- 状态：调查中（应用侧症状链已确认；设备固件 / Windows BLE HID 栈的根因未定）
- 影响范围：Windows 10.0.19045（`fast_startup_enabled=true`，`boot_type=hiberboot`）；
  SayAll 1.9.3（`source_revision=a4b5512…`）；遥控器型号 `Rc001`（GATT 2A24）；
  「全按键支持」开启；语音输入工具为豆包
- 功能点：RC003 增强捕获助手 / RC001 报告层 / ATVV 语音会话 / BLE 重连
- 现象：连接遥控器后，按语音键只有第一次出现语音条；松开后再按没有反应，
  随后遥控器从 Windows 消失，应用进入长时间「正在连接遥控器」，日志里
  `发现 ATVV 服务返回状态 1`（`Unreachable`）反复出现，直到应用退出
- 复现条件：RC001 + 增强捕获开启 + 豆包；按住语音键说话一次后松开，再按一次
- 正常预期：每次按下都开始一次 ATVV 会话；松开时收到 `STREAM_STOP` 或由
  8 秒看门狗收尾，下一次按下仍能开始新会话；HID 报告持续到达

## 证据（用户提供的 `sayall-diagnostic.log`，时间均为 UTC；括号内为本地 +08）

### 首次也是唯一一次成功按下：2026-11-14T16:37:02Z（11-15 00:37:02）

- `16:37:02.283` agent `synth:frame seen` → `synth:frame replace ok to=0xe6`
- `16:37:02.311` `voice_attempt session=1 tool=doubao path=report_layer synth=0x00E6 synth_confirmed=true`
- `16:37:02.416` `audio_stream phase=started result=passed endpoint_kind=virtual_cable`
- `16:37:11.899` `voice_session action=abort terminal_result=failed reason=audio_stall
  session=1 samples=23760 silent_ms=8012 note=stop_not_received`
  - 23,760 samples ≈ 1.485 s 音频；之后既没有音频，也没有 ATVV `00` 停止帧
  - 该次按下**没有** `synth:frame release hold_ms=200`（对比成功的会话都有这一行），
    说明语音键的 HID 释放报告从未到达宿主

### 遥控器从系统消失

- `16:37:11.920` `ble_link phase=changed connection_generation=30 status=BluetoothConnectionStatus(0)`
- `16:37:13.015` `raw_input binding_audit action=unbound reason=device_missing`
- `16:37:13.908` `wake_reconnect triggered=device_arrived`
- `16:37:23.015` `raw_input binding_audit action=present matched_device_count=1`
  - 设备节点回来了，但之后**零 HID 报告**：`shortcut_capture action=diag` 一直是
    `keys_seen=0`，也没有 `raw_input stale_remote_event`

### 重连一直 Unreachable

- `16:37:24.098` 首次 `async_wait label=gatt_find_atvv_service phase=completed
  terminal_result=failed reason=timeout timeout_ms=10000 action=cancel`
- 同样超时持续到 `17:00` 应用退出；中间每次 `ble_connect_stage phase=failure_detail
  stage=service_discovery raw_error=发现 ATVV 服务返回状态 1`
- 应用侧 `ble_reconnect` 退避正常推进，但从未成功

### 第二天重启应用（2026-11-15T06:11:45Z / 14:11:45）

- `06:11:56.719` `ble_connect phase=completed terminal_result=passed` —— GATT 链路能建立
- 助手 `[HOST] pid=1472`、`[TAP] scan=ok modules=44 resident=1`、
  `[ATTACH] action=skip_injection` —— 复用了宿主里已有的 agent
- `06:11:54.145` `enhanced_capture event=ownership_resumed`
- 但整个 13.6 小时里助手累计：`synth_hits=1`、`target_hits=0`、`edges_sent=0`、
  `voice_clear_hits=0`；`ioctl_calls` 只按空闲基线增长（约 1 次/12 秒），
  没有任何按键报告突发
- `06:13:10.217` `capture_inactive reason=agent_never_acked elapsed_ms=75259`
- `06:13:12` 应用自动重启助手；新助手仍 `[TAP] resident=1` / `[ATTACH] skip_injection`，
  再次接回同一个宿主 agent
- `06:13:53.141` 再次 `voice_synth_gate active=false reason=ownership_lost`；
  `06:13:54.576` agent `stall pump_age_ms=1635`；之后恢复所有权
- 日志结束于 `06:14:01`，仍没有任何 HID 报告

### 启动类型

- 故障前后两次应用启动都是 `boot_type=hiberboot fast_startup_enabled=true`
- 在这台机器上，「关机再开机」不会完整复位蓝牙栈；与
  `DEBUGGING.md` 2026-09-16 的恢复矩阵一致（只有完整重启清除了僵死态）

### 型号与路径

- 所有 `ble_model` 读数都是 `model=Rc001`，不是 RC003
- 增强捕获助手按共享 VID/PID 匹配到了这台 RC001，并执行了
  `synth=0x003e->0x00e6`

## 已确认与未确认

### 已确认

1. 现象发生在 HID 报告层之下：首次语音按下后，遥控器的 HID 报告流
   （语音键 + 后续所有键）与 ATVV `STREAM_STOP` 同时消失。
2. 应用侧 8 秒 `audio_stall` 看门狗按设计收尾，但它只是症状处理；
   收尾时遥控器已经不再上报。
3. 遥控器 HID 设备节点会消失再出现；重新出现后仍然零报告。
4. GATT 重连在 25 分钟内持续 `Unreachable`；第二天应用重启后 GATT 能连上，
   但 HID 报告仍未恢复。
5. 该状态跨应用重启、跨 fast-startup 关机/开机存活。

### 未确认

1. 是遥控器固件在首次语音释放时挂死，还是 Windows BLE HID 栈/设备节点僵死。
   现有应用日志无法区分；需要应用关闭状态下的 HID 探针或系统事件日志。
2. 助手宿主是否在设备重新枚举后换了 `WUDFHost`，而助手仍服务旧宿主。
   日志里助手 `[HOST] pid=1472` 在第二天仍是同一个值，但无法证明中间没有换过。
3. 增强捕获的报告层改写是否是触发条件。首次按下确实走了
   `synth=0x003e->0x00e6`，但没有 A/B（关闭「全按键支持」）对照。
4. RC001 与 RC003 的行为是否相同；本次只有 RC001 现场。

## 应用侧缺口（按优先级）

1. **无 HID 报告活性判据**：`raw_input` 只判设备路径是否在位，不判报告是否
   还在到达；助手心跳也没有「最近一次真实报告时间」。界面可能显示已连接，
   按键与语音键却全死。**本次已补**（见下）。
2. **助手只按目标地址重新解析宿主**：`retarget_watch_loop` 只在
   `rc003-target.ini` 的地址变化时重启自己；同一支遥控器重新枚举
   （实例后缀在日志中出现过 `27e2ec11` / `16dd0531` / `332d757f` /
   `b927bc3` / `2dd34e6`）不会触发重新解析。**仍未修**。
3. **自愈只重启助手，不换 agent 世代**：`capture_inactive` 的 `/end` + `/run`
   在宿主仍有 resident agent 时走 `[ATTACH] skip_injection`，接回同一个 agent；
   若该 agent 的报告路径已失效，重启助手是空操作。**仍未修**。
4. **诊断日志含路径**（独立问题，见下）：`raw_input binding_initial`、
   `binding_audit action=rebound|unbound`、`stale_remote_event` 会打印完整 HID
   路径；`rc003_bridge phase=listening` 会打印完整 `%LOCALAPPDATA%` 路径。
   用户提供的日志因此包含用户名与设备实例路径，违反 `LOGGING.md` 隐私红线。
   **本次已修**（见下）。

## 本次改动（诊断 + 脱敏，2026-11-15）

### 报告流活性

- agent `AGENT_BUILD` 升至 `2026-11-15.report-liveness`：
  - 新增 `stat.reports_seen` 与 `lastReportAt`：只要宿主收到形状匹配
    （IOCTL + 8/9 字节 + `report_id=0x01`）的 HID 报告就计数，**不看 usage、
    不看来源**；心跳新增 `last_report_age_ms`。
  - 这样下一次现场可以直接回答“宿主里还收得到报告吗”，而不是从
    `target_hits` / `synth_hits` 反推（这两个只在命中目标时才动）。
  - **读法边界**：`reports_seen` 是宿主级，不是遥控器级；共享宿主里另一台
    HID 设备也可能让它增长。`reports_seen` 涨但 `foreign_reports` 也涨 =
    只收到别的设备；`reports_seen` 不涨而 `ioctl_calls` 涨 = 报告形状/IOCTL
    变了，不是“没收到”。
  - **升级首次运行**：宿主里的旧 agent 会被识别为 stale，现有刷新路径会换代；
    共享宿主走 RC003 设备节点恢复，可能耗时数十秒。验收前先确认
    `[HELLO] ... build=2026-11-15.report-liveness`。
- 助手 `[HB]` 新增 `last_report_age_ms`、`host_pid`，并把 `reports_seen`
  随 `stat` 一起落盘；设备重新枚举后可直接对比服务宿主有没有换。

### 应用侧报告流

- `raw_input` 新增 `last_report_at` 与纯函数 `report_flow_gap_ms`：首条报告或
  距上一条 ≥ 5 秒时落一行
  `raw_input report_flow first=… gap_ms=… raw_event_count=…`。
  设备在位但报告流已停时，这一行会比“没有报文”多一个可判定的时间事实。
  只对 `accepted` 报告生效；路径失配的报告由 `stale_remote_event` 首次行覆盖。
- `binding_audit action=rebound` 去掉路径，改为 `binding_changed` /
  `previous_binding_present` / `path_changed` 三个布尔值：
  `path_changed=true` 才是真的换了路径；同一实例掉线又回来为 false。
- `raw_input binding_initial`、`binding_audit action=timer_armed|ambiguous` 去掉路径；
  `rc003_bridge phase=listening` 只记 `descriptor=published|none`，不记描述文件路径。
- `stale_remote_event first_seen` 去掉 `bound_path` / `event_path`，改为
  `target_known=true|false`、`address_matches_target=true|false`：
  后者为 false 且 target 已知 = 报文来自另一支遥控器；target 未知时只能靠计数。

### 助手侧脱敏

- 新增 `display_file_name`：`[GADGET] src`、`[VERIFY] file`、`[HOST] image`、
  `[TAP] path`、`[ATTACH] path`、`[WARN] 不在运行时目录` 只记文件名；
  `[TAP] path` 另带 `in_runtime_dir=true|false`。具体是哪一份二进制由
  `[VERIFY]` / `[DLL]` 的 SHA-256 证明。
- `%ProgramData%` / `%LOCALAPPDATA%` 形态的路径仍按 `LOGGING.md` 允许保留
  （只保留定位变量，不保留用户名）。

### 验证

- `node hardware/RC003/helper/agent/agent_logic_test.mjs`：**99/99 passed**
  （新增报告流活性 4 项）。
- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：**87 passed / 0 failed**
  （新增 `log_file_identity_never_carries_parent_paths`）。
- `sayall-helper.exe --selftest --no-app-bridge`：**passed**（内嵌 agent
  `AGENT_BUILD=2026-11-15.report-liveness` 一致；agent SHA-256
  `fe60d6ca…`）。
- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows --lib`：**564 passed / 17 ignored / 0 failed**
  （当前工作区；包含并行未提交修改新增的用例；本次新增
  `report_flow_gap_logs_first_and_long_gaps_only`）。
- `cargo check --workspace`：passed。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed。
- **真机复现 + 本地测试包：deferred**。需要按 E1–E5 在 RC001 真机上复现；
  本次包尚未构建/安装。

## 下一步（最小实验矩阵）

| 编号 | 实验 | 判据 |
| --- | --- | --- |
| E1 | 关闭应用，用既有 HID 探针 / helper `observe` 模式按语音键与音量键 | 仍无报告 ⇒ 问题在 HID 链路/设备；有报告 ⇒ 应用或助手绑定问题 |
| E2 | 记录故障前后 `HostPid` 与设备实例后缀；对比助手 `[HOST] pid` | 实例或宿主变了而助手未重解析 ⇒ 缺口 2 成立 |
| E3 | 对遥控器 HID 节点做 `pnputil /remove-device` + `/scan-devices`（助手已有该路径） | 报告恢复 ⇒ 可作为自愈动作；仍无 ⇒ 继续 E4 |
| E4 | 完整「重启」（不是关机再开机），复测 | 恢复 ⇒ Windows BLE 栈僵死；不恢复 ⇒ 更像遥控器固件 |
| E5 | 关闭「全按键支持」，重复首次语音后松手 | 不再断连 ⇒ 报告层改写/助手路径是触发条件之一 |

## 隐私检查

本文不含用户名、完整路径、蓝牙地址、HID 路径、设备实例、语音内容或凭据。
用户提供的原始日志本身含上述信息，按敏感材料处理，不应再转发或提交。
