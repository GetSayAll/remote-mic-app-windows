# Chromecast Remote Windows 真机验收

## 范围与设备

- 目标设备：Chromecast Remote（Google，蓝牙 VID `0x18D1` / PID `0x9450`，
  `2A24` 型号 `A3`）。
- 语音：ATVV v1.0、codec `0x02`（16 kHz）、帧长 240；按下语音键由遥控器直接发
  `AUDIO_START(0x04)`，松开发 `AUDIO_STOP(0x00)`；语音键不产生 HID 报文。
- 按键：BLE HID 集合 Col01，`Report ID 0x01`、3 字节 `01 <code> 00`；
  码表见 `docs/investigations/evidence/2026-09-18-chromecast-remote-atvv-hid-probe.md`。
- 本手册只适用于真实 Windows 主机 + 真实遥控器；CI、仿真或 Mac 上的结果
  不能表述为本手册通过。

## 前置

1. 安装本仓库构建的本地测试包（NSIS）。
2. 在 Windows「蓝牙和其他设备」中确认 `Chromecast Remote` 已配对并连接。
3. 语音输出端点选择 `CABLE Input (VB-Audio Virtual Cable)`，系统默认录音设备
   切到 `CABLE Output`；如需验文字上屏，先启用目标输入法的语音热键。
4. 从托盘正常启动应用（不要强杀正在连接的应用）。

## 用例

### 1. 型号识别与连接

1. 连接页扫描已配对设备。
2. 选择 `Chromecast Remote`。

预期：列表显示该设备；连接页型号显示「Chromecast Remote（谷歌）」；
连接阶段依次推进到「已连接」，能力显示 16 kHz ATVV。

失败判定：扫描不到；型号显示为 unknown；能力确认失败或显示 8 kHz。

### 2. 首次语音（核心）

1. 配置按住说话快捷键（或先保持「关闭」分别验音频）。
2. 按住语音键（Google Assistant 键）约 2 秒并说话，松开。
3. 再重复 2 次。

预期：按下注入快捷键 DOWN、松开注入 UP，严格成对；`STREAM_START → AUDIO →
STREAM_STOP` 完整；解码采样数与语音会话次数增加；音频可听、无明显每帧爆音；
松手后无粘键。

### 3. 快速连按与空会话

1. 快速连按语音键 5 次（每次 <300ms）。

预期：每次按下/释放成对；不残留按住的快捷键；不产生重复音频；会话代次只增
不串。

### 4. 普通按键

依次按：确定 / 上 / 下 / 左 / 右 / Home / 静音 / YouTube / Netflix / 电源 /
输入源。

预期：已配置动作按单击/双击/长按生效；未配置按键保持原样；无「原生 + 映射」
双响应；按住不产生遥控器自带连发（连发由应用侧手势决定）；无粘键。

### 5. 断连、睡眠与恢复

1. 语音会话中让遥控器断电或移出范围。
2. 触发一次 Windows 睡眠再唤醒。
3. 恢复后立即再按一次语音键。

预期：断连/睡眠先统一释放快捷键与音频；旧会话回调不改变新会话；恢复后第一次
语音正常，无需重新配对、重启蓝牙或重启应用。

### 6. 与小米遥控器共存

在 RC001/RC003 与本设备之间切换所选遥控器，各触发一次语音与两个普通键。

预期：Raw Input 只绑定当前所选型号的 HID 接口；切换后按键归因正确，不出现
跨型号误触发或双输入。

## 证据与结论

- 使用 `SAYALL_GATT_LOG` 诊断日志作为 ground truth，记录会话号、连接阶段、
  `chord_press` 成对、音频 begin/finish 与按键 `map_fire`。
- 结论按 `passed` / `failed` / `deferred` 记录，并注明执行主机与遥控器固件版本。
- 自动化（`cargo test --workspace`、前端测试、仿真）只证明代码路径，不替代本手册。
