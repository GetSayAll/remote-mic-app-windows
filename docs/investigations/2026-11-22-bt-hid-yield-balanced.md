# 蓝牙键鼠共存：让出时隙后遥控器语音恢复满速（2026-11-22 实测）

## 结论

- 三链路共存（遥控器 + 蓝牙键盘 + 蓝牙鼠标）时，遥控器语音只有约 5.2–5.6 k samples/s。
- 只把键盘或只把鼠标切到 60 ms / 180 ms，**都不能恢复**（4.2–5.0 k/s）。
- 把键盘和鼠标**同时**切到 `Balanced`（60 ms）或 `PowerOptimized`（180 ms），遥控器语音恢复到 **15.4–15.5 k samples/s（满速）**。
- 语音结束或实验结束后，`ThroughputOptimized` + 释放设备句柄，键鼠恢复到原来的 7.5 ms / 延迟 44；没有永久修改配对或设备配置。
- 因此这是**可产品化的软件层解法**：不是关闭设备，而是让键鼠在语音期间让出蓝牙连接事件。产品决定：**只在语音输入会话期间自动让出，语音结束立即恢复**；平时不占用键鼠链路，避免鼠标平时卡顿。没有用户开关。本机实测 `apply` 成功在 146–214 ms，2.658 s 的短会话整段 14,898 /s，未观察到“前 1–2 s 降级”；其它适配器仍 `deferred`。

## 环境与方法

- SayAll 1.9.3（`source_revision=09b823f…`）初始 A/B；最终自动实现用 1.9.4
  （`source_revision=2813b07…`）本地测试包复验。Windows 25H2 build 26200，
  MediaTek 蓝牙适配器。
- 遥控器、蓝牙键盘、蓝牙鼠标同时 Connected；键盘/鼠标均为 BLE HID。
- 只使用公开 WinRT API：
  - `BluetoothLEDevice.FromBluetoothAddressAsync`
  - `BluetoothLEDevice.GetConnectionParameters`
  - `BluetoothLEDevice.RequestPreferredConnectionParameters`
  - `BluetoothLEPreferredConnectionParameters.Balanced` / `PowerOptimized` / `ThroughputOptimized`
- 语音速率取应用日志 `voice_audio_rate`；参数取 `GetConnectionParameters` 的 `ConnectionInterval` / `ConnectionLatency` / `LinkTimeout`。
- 实验脚本不写入设备名或地址到本文；实验后设备已恢复。

## 预设值（本机只读回）

| 预设 | Min interval | Max interval | Latency | Link timeout |
| --- | --- | --- | --- | --- |
| `Balanced` | 24（30 ms） | 48（60 ms） | 0 | 400（4 s） |
| `PowerOptimized` | 72（90 ms） | 144（180 ms） | 0 | 600（6 s） |
| `ThroughputOptimized` | 12（15 ms） | 12（15 ms） | 0 | 200（2 s） |

## A/B 结果

| 场景 | 键盘参数 | 鼠标参数 | 遥控器语音 |
| --- | --- | --- | --- |
| 基线（键鼠默认） | 6 / 44 / 216（7.5 ms） | 6 / 44 / 216（7.5 ms） | 5.2–5.6 k/s，degraded |
| 只降键盘（PowerOptimized） | 144 / 0 / 600（180 ms） | 6 / 44 / 216 | 4.2 k/s，degraded |
| 只降鼠标（Balanced） | 6 / 44 / 216 | 48 / 0 / 400（60 ms） | 4.8–5.0 k/s，degraded |
| 键盘 + 鼠标 Balanced | 48 / 0 / 400（60 ms） | 48 / 0 / 400（60 ms） | **15,539 /s，full**（3.475 s） |
| 键盘 + 鼠标 PowerOptimized | 144 / 0 / 600（180 ms） | 144 / 0 / 600（180 ms） | **15,442 /s，full**（2.44 s） |

## 恢复路径

1. 请求 `ThroughputOptimized`：两只设备读回 12 / 0 / 200（15 ms）。
2. 释放 `BluetoothLEPreferredConnectionParametersRequest` 与 `BluetoothLEDevice` 句柄。
3. 重新只读回：两只设备均为 **6 / 44 / 216（7.5 ms / 延迟 44）**，即 HID 栈原来的参数。
4. 遥控器不受影响，仍为 12 / 0 / 200。

## 解释

- 键盘/鼠标没有麦克风，但它们各占一条 BLE 链路和连接事件；适配器按事件轮流调度，遥控器 ATVV 音频一帧一个事件（120 B = 240 samples = 15 ms）。
- 只让一条 HID 链路让出时隙，另一条 7.5 ms 链路仍占住调度，遥控器仍只拿到约 1/3。
- 两条 HID 链路都让出后，遥控器可以拿到足够的连接事件；60 ms 已足够，不必到 180 ms。

## 产品含义与风险

- 只做「语音优先」自动行为：只在遥控器 `Streaming`（语音流）期间把已连接的 BLE HID 键鼠设为 `Balanced`；语音结束、遥控器断开或应用退出时恢复。**没有设置项、没有用户开关**。
- 60 ms 比 180 ms 更跟手，实测已满速；优先选 `Balanced`。
- 代价：仅在语音流期间，键鼠响应间隔变为 60 ms，可能略增耗电；平时保持 7.5 ms 原参数。
- 若只在语音会话开始时切换，参数协商理论约 2 s；本机 1.9.4 两次会话实测
  `apply` 成功在 146–214 ms、2.658 s 短会话整段 14,898 /s，未观察到前 1–2 s 降级；
  其它适配器仍需复测。
- 失败兜底：`Status=AccessDenied/DeviceNotAvailable`、非 HID 设备、非 Windows 11 22000+ 都只记日志并继续正常语音，不阻塞、不回滚主链路。
- 只处理已连接的 BLE HID 设备，排除遥控器自身；不读取/不修改任何第三方 App 数据。

## 2026-11-22 最终实现（自动、仅语音流期间）

- 设置：不新增设置项，`AppSettings` schema 保持 7；没有 `voice_priority_enabled`，没有 IPC set 命令。用户不需要开启或关闭。
- 运行时：`VoicePriorityRuntime` 每 250 ms 读取连接快照；仅当 `phase == Streaming` 且遥控器目标存在时，发现已连接的 BLE HID 设备（按地址排除遥控器），请求 `Balanced` 并持有句柄；其他所有阶段（Ready/Draining/Disconnected/Failed）请求 `ThroughputOptimized`、释放句柄并把状态置回 `off`。语音中途结束时，申请过程中会再次检查 `Streaming` 并放弃本次让出。
- 去重与刷新：按蓝牙地址去重；设备发现结果缓存 30 s，失败 5 s 后重试。
- 状态：`VoicePrioritySnapshot { state, yieldedDevices }` 进入 `PlatformSnapshot.voicePriority`；界面只显示用户语言，不显示地址/设备名/内部参数。
- 界面：连接页不显示「语音优先」区块、开关或只读说明；自动行为无额外界面文案。
  降级提示仍解释语音输入期间会自动让出、结束后恢复，保留「忽略此提醒」。
- 手工探针（`cargo run -p sayall-windows --example voice_priority_probe`，2026-11-22 实测）：
  - 模拟 `Streaming`：`state=active yielded=2` 持续 8 s；
  - 切回 `Ready`（语音结束）：3 s 内 `final state=off yielded=0`，键鼠参数已恢复。
- **1.9.4 本地测试包安装后实机验证（RC003 + 蓝牙键盘 + 鼠标）**：
  - 会话 A（2.658 s）：`voice_begin` → 118 ms 后 `state=applying` → 214 ms 后 `apply` 成功；
    `audio_stream started` 在 145 ms；`voice_audio_rate result=full rate_hz=14898`。
  - 会话 B（15.759 s）：`voice_begin` → 76 ms 后 `state=applying` → 146 ms 后 `apply` 成功；
    `audio_stream started` 在 195 ms；`voice_audio_rate result=full rate_hz=15777`。
  - 独立只读探针（每 500 ms）：会话期间键盘/鼠标 `48 / 0 / 400`（60 ms），遥控器始终
    `12 / 0 / 200`；松开后 `restore` 成功、`state=off`，键盘/鼠标回到 `6 / 44 / 216`。
    复现命令：`cargo run -p sayall-windows --example hid_params_read_probe -- 60`
    （只输出序号与参数，不含地址/设备名）。
  - 结论：本轮没有观察到“前 1–2 s 仍降级”。2.658 s 的短会话整段平均 14,898 /s；
    若前 2 s 只有 ~5 k/s，整段平均不可能达到这个值。`apply` 请求在两段会话中都在
    音频流开始前后 100 ms 内完成（会话 A 的成功回执比 `audio_stream started` 晚
    69 ms，会话 B 早 49 ms）；真实生效点由上述只读探针的 60 ms 读回确认。
  - 该包为本地测试构建，不是发布包；RC001 与其它适配器仍 `deferred`。
- 中间实现（已废弃，保留证据）：曾做成「默认关闭、降级后一键开启」的常驻设置。1.9.3 本地测试包安装后，开启后 `candidates=2 yielded=2` / `state=active`，只读回键鼠 `48 / 0 / 400`；两次语音会话 `voice_audio_rate result=full rate_hz=15211`（2.761 s）与 `14193`（2.046 s）。该证据证明机制有效，但产品行为已按用户要求改为自动、仅会话期间。
- 自动化：前端 45 个测试文件、719 passed / 10 skipped；`vue-tsc --noEmit` + `vite build` 通过；`cargo test --workspace` 通过（含 `voice_priority` 2 passed）；`cargo fmt --all -- --check` 通过；`cargo check -p sayall-windows-app --features runtime-simulation` 通过；`scripts/ci-preflight.ps1` 7/7 通过。

## 未覆盖

- 仅本机、本适配器、RC003；RC001 与其它适配器 `deferred`。
- 端到端文字上屏未做独立 OCR 取证；满速会话的样本率已实测。
- 只测了两次会话（2.658 s / 15.759 s），没有逐秒拆分样本率；短会话整段 full 已排除
  “前 2 s 固定降级”的假设，但更多适配器/时序组合仍需复测。
- 键鼠耗电变化未测。
