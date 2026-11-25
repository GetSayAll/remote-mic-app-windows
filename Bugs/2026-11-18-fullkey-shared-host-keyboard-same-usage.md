# 全按键支持：共享 WUDFHost 内蓝牙键盘的方向/回车被当成遥控器按键

- 发现日期：2026-11-18（客户反馈，微信「无线麦 SayAll.App」20:39）；复核：2026-11-25
- 状态：**调查完成，属已登记的设计内边界**；等待产品决策（`TODO.md` 全按键接管名单），本轮未改代码
- 影响范围：1.9.3（`a4b5512…`）与 1.9.5（`4ead96e…`）同一台机器；bookpro14 / Windows 11（
  `windows_build=26200`）；RC003（`pid&32b8`）；蓝牙键盘（与遥控器同处一个 `WUDFHost.exe`）；
  1.9.7（main `011aeba`）代码复核仍在
- 功能点：按键页「全按键支持」→ `rc003_bridge` 目标集下发 → Helper / agent 报告层按 usage 清空
- 现象：开启「全按键支持」后，蓝牙键盘的上下左右 / 回车被识别成遥控器的方向 / 确定，
  按键盘方向键会执行遥控器方向键配置的动作（翻页 / 滚屏 / 鼠标移动）；关闭「全按键支持」后
  冲突消失，但遥控器的返回 / 音量± 按设计不可用
- 复现条件：同一 `WUDFHost.exe` 同时承载 RC003 与物理键盘；遥控器方向 / OK 配置了动作（
  `mapped_mask` 命中）
- 正常预期：物理键盘的任何键都不应被当成遥控器输入

## 证据

### 1. 客户日志（2026-11-18，1.9.3，UTC；本地 = UTC+8）

20:33:13（`12:33:13Z`）全按键支持开启，报告层接管 12 个 usage，包含方向 / OK：

```text
12:33:13.522Z enhanced_capture event=targets_changed generation=17 enabled=true
              usages=28,35,4a,4f,50,51,52,65,66,80,81,f1
12:33:15.907Z enhanced_capture event=ownership_resumed
              usages=28,35,4a,4f,50,51,52,65,66,80,81,f1 persistent_swallow=0
```

同一分钟遥控器方向 / OK 的边沿与动作全部来自报告层（门控吞键数为 0）：

```text
12:33:20.310Z map_edges count=1 detail=Up=true gate(sw=0 lk=122)
12:33:20.314Z map_fire button=Up trigger=Single action=page_up
12:33:21.579Z map_fire button=Down trigger=Single action=page_down
12:33:21.933Z map_fire button=Left trigger=Single action=scroll direction=Up steps=20
12:33:22.240Z map_fire button=Right trigger=Single action=mouse_move direction=Right distance=20
12:33:23.003Z map_fire button=Ok trigger=Single action=to_top
```

20:34:32（`12:34:32Z`）客户关闭「全按键支持」，客户 20:39 反馈“关了以后键盘不再冲突”：

```text
12:34:32.653Z enhanced_capture event=ownership_released fallback=legacy persistent_swallow=0
12:34:32.653Z enhanced_capture event=targets_changed generation=18 enabled=false usages=-
```

### 2. 宿主不是独占：报告层与另外两路 BLE HID 共用 `WUDFHost.exe`

```text
12:33:13.584Z [EXCLUSIVE] members=3 rc003_members=1 verdict=shared_host
12:33:13.584Z     [其它] index=1 enumerator=BTHLEDevice class=other_device
12:33:13.584Z     [RC003] index=2 enumerator=BTHLEDevice class=rc003_hid_service_match
12:33:13.585Z     [其它] index=3 enumerator=BTHLEDevice class=other_device
12:33:13.585Z [SHARED-HOST] decision=continue_with_usage_gate members=3 rc003_members=1
```

日志按隐私红线不记设备身份/路径，因此“这两路就是那把蓝牙键盘”无法仅凭日志坐实；
同机制的内置键盘现场已在 `Bugs/2026-11-15-shortcut-capture-enhanced-capture-suspension.md` 确认。

### 3. 1.9.5 仍在接管同一批键（11/25 `04:21:47Z`）

```text
enhanced_capture event=targets_changed generation=2 enabled=true
  usages=28,35,4a,4f,50,51,52,65,66,80,81,f1
enhanced_capture event=ownership_resumed usages=28,35,4a,4f,50,51,52,65,66,80,81,f1
```

## 根因

1. 报告层按**宿主**挂 `NtDeviceIoControlFile`，不是按设备；判据只有报告形状
   （IOCTL `0x80018483`、8 字节入参、`report_id=0x01`）+ 内容 usage
   （`hardware/RC003/helper/agent/rc003_agent.js:939-993`）。
2. RC003 的方向 / OK 使用标准键盘页 usage（`0x4F`–`0x52`、`0x28`），物理键盘的
   同名键完全同形。agent 的空闲来源判定是“第一份含目标 usage 的报告认领来源”
   （`rc003_agent.js:958-962`），键盘先按时会被当成遥控器；随后其报告在报告层被
   清空（`rc003_agent.js:1049`）并作为遥控器边沿上报。
3. `NtQueryObject` 得到的来源名在共享宿主内是宿主自己的反射控制设备
   （`\Device\UmdfCtrlDev-{GUID}`），同一宿主所有成员共用——**来源隔离（2026-11-12）
   在这种宿主里退化为恒等**，不能区分键盘与遥控器。PR #385 的真机采样
   （`4573633`）已证明这一点，因此 PDO 白名单方案被停用，不能作为修复。
4. 目标集 = 已配置映射的键 ∪ 返回/音量±（`crates/sayall-windows/src/rc003_bridge.rs:989`）。
   客户为方向 / OK 配了动作，于是它们进入报告层名单；所有权 ACK 后 `key_gate`
   对这些键停用旧全局门控（`crates/sayall-windows/src/key_gate.rs:549-560`），
   应用只能看到报告层边沿，无法再区分物理来源。
5. 关闭「全按键支持」后，只剩旧路径：Raw Input 按设备路径过滤
   `vid_2717/pid_32b8`（`crates/sayall-windows/src/raw_input_windows.rs:854-912`），
   LL 门控只在遥控器 HID 报文武装后的 60ms 窗口内吞同名键——键盘只在窄竞态里受影响，
   所以客户观察到的“关闭后不冲突”。代价是返回/音量± 本就到不了系统，保持不可用。

结论：**最新代码（main `011aeba`，v1.9.7）仍有此问题**；这是报告层“按宿主而非按设备”
的已知边界，不是 1.9.x 新引入的回归。

影响型号：RC001 与 RC003 共用同一 BLE HID 硬件串（`vid&012717_pid&32b8_rev&00a4`），
增强捕获助手对两型号都会匹配并接管报告层；本结论对 **RC001 与 RC003 均成立**。
证据矩阵见 [`docs/investigations/2026-11-25-rc001-rc003-key-path-evidence.md`](../docs/investigations/2026-11-25-rc001-rc003-key-path-evidence.md)。

## 已尝试且不可行的修复

- PR #385（branch `review-pr-385-20261121`）：helper 解析 RC003 及 HID 子节点 PDO 名下发
  agent 白名单。真机证明钩子来源统一是 `\Device\UmdfCtrlDev-{GUID}`，非空白名单会把
  遥控器报告全部外拒（返回键整体失效）。后续提交 `4573633` 停用白名单，最终
  `pdo_sources` 恒为空（`hardware/RC003/helper/src/main.rs`），**不构成修复**；PR 仍未合并。

## 已实施的修复（2026-11-25，Andy 拍板）

采用“共享宿主感知 + 只排除高频键”的组合，两型号统一处理：

- helper 在 `HELLO` 追加 `host=... members=... rc003_members=... other_keyboard=...`；
  `other_keyboard` 通过其它成员子 HID 节点的 `HardwareIds`
  （`HID_DEVICE_SYSTEM_KEYBOARD` / `HID_DEVICE_UP:0001_U:0006`）判断，
  不读设备身份、不写日志。
- bridge 目标集：独占宿主 / 确认无键盘 / 旧助手无判决 → 全键；
  共享宿主且存在键盘类成员（或检测未知）→ 去掉 OK/Enter（`0x0028`）与方向
  （`0x004F`–`0x0052`），返回 / 音量±、Home / TV / Menu 等保持原策略。
- 高频键回落到旧 Raw Input + key_gate；键盘冲突只剩 60ms 武装窗口的窄竞态。
- 两型号共用 BLE HID 硬件串，`ble_model` 只在应用层区分；策略不按型号分支。
- 新增日志：`enhanced_capture event=remote_model`、`event=host_policy`、
  `event=targets_changed`（均带 `model=`），helper 侧 `[EXCLUSIVE]` /
  `[APP-BRIDGE] event=hello_config` 带 `other_keyboard` / `host_policy`。

未采用：PR #385 的 PDO 白名单（真机已否证）；按设备识别仍留作研究项。

## 验证

- 自动化：`sayall-windows` lib **600 passed / 20 ignored**、helper **92 passed**；
  `cargo fmt --all -- --check`、helper fmt、`cargo check -p sayall-windows-app --features
  runtime-simulation` passed。
- 本机 helper dry-run：`[EXCLUSIVE] members=1 rc003_members=1 verdict=exclusive_rc003_host
  other_keyboard=0 host_policy=full`，独占路径不收紧。
- **deferred**：共享宿主 + 键盘的真机验证（本机为独占宿主）；需 RC001 / RC003 各一次，
  按 `Testing/WindowsRC003EnhancedCapture.md` 的 13 键回归执行。

## 隐私检查

只使用时间戳、会话计数、usage 号、进程号与稳定事件名；未包含设备名、蓝牙地址、
HID 路径、用户文件路径或语音内容。
