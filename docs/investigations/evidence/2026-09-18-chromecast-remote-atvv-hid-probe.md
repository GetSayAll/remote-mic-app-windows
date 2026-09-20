# Chromecast Remote ATVV/HID 真机探测实证（2026-09-18）

## 目标

评估能否在 Windows 版新增对 Chromecast Remote（Google 参考设计遥控器）的支持。
判定语音链路（ATVV）是否满足 SayAll 的 16 kHz 门槛，以及普通按键的 HID 形态与用法。

## 设备与工具

- 设备：`Chromecast Remote`，Google 厂商 `VID 0x18D1` / `PID 0x9450`，
  标准 `2A24` 型号串 `A3`，厂商 `Google Inc.`，硬件 `CR PVT/MP`，固件 `26.5`。
  已在 Windows 蓝牙中完成配对并保持连接。
- 工具（本仓库 examples，仅诊断、不写入产品代码路径）：
  - `crates/sayall-windows/examples/atvv_probe.rs`：枚举 ATVV 服务接口，订阅
    AUDIO/CONTROL，向 TX 写 `GET_CAPS`，打印 capabilities 与语音键 CTL 事件。
  - `crates/sayall-windows/examples/hid_probe.rs`：Raw Input（键盘 `0x01:0x06`、
    Consumer `0x0C:0x01`、厂商页 `0xFF00:0x01`）零过滤打印遥控器报文。
- 隐私：两工具都不打印真实蓝牙地址与完整 HID 实例路径。

## GATT 服务形态

ATVV 服务与特征角色与现有实现完全一致：

```text
AB5E0001-5A21-4F05-BC7D-AF01F617B664  Service
AB5E0002-...  TX       WriteWithoutResponse
AB5E0003-...  AUDIO    Notify
AB5E0004-...  CONTROL  Notify
```

## ATVV 结果（passed）

`GET_CAPS` 成功后约 198 ms 收到 `GET_CAPS_RESP`：

```text
GET_CAPS_RESP: version=0x0100 codecs=0x02 interaction=0x03
               frame_size=240 selected_codec=0x02 sample_rate=16000
               supports_sayall_audio=true
```

语音键按下/释放（40 秒内 3 次会话，共 277 个 AUDIO 通知）：

```text
[AUDIO_START 0x04 03 02 01] session_id=1 codec=2     按键按下
[AUDIO #1 len=240 ...]                               240 字节 ADPCM 通知
[C] 00 02                                            按键释放（AUDIO_STOP）
[C] 04 03 02 02 ... 00 02                            下一次按下/释放
```

结论：

- ATVV 版本 1.0，codec `0x02`（16 kHz），交互模型 `0x03`（PTT + HTT），
  帧长 240 字节，`supports_sayall_audio()` 为真 → **满足现有采样率门槛，核心解码无需改动**。
- 遥控器在按下时直接发送 `AUDIO_START`，无需主机先发 `MIC_OPEN`
  （未观察到 `START_SEARCH 0x08`）。
- 松开时发送 `AUDIO_STOP 0x00`（带 reason 字节），按下/释放语义完整，
  符合"按住开始、释放结束"的实时生命周期，无需双击等待或长按阈值。
- 未观察到 `DECODER_SYNC 0x0A` 事件。

## HID 结果（passed）

- 遥控器暴露两个 HID 接口：`Col01`（Windows 归类 Consumer Control）与
  `Col02`（Vendor-defined）；所有按键事件都落在 `Col01`。
- 报文形态：`Report ID 0x01`、固定 3 字节、格式 `01 <code> 00`，松开为 `01 00 00`。
- 按键码（用"确定"作分隔符逐键确认）：

| 按键 | code | 按键 | code |
| --- | --- | --- | --- |
| 确定 | `0x07` | 返回 | `0x0B` |
| 上 | `0x03` | Home | `0x0A` |
| 下 | `0x04` | 静音 | `0x08` |
| 左 | `0x05` | 音量+（右侧实体键） | `0x0C` |
| 右 | `0x06` | 音量−（右侧实体键） | `0x0D` |
| 电源 | `0x01` | YouTube | `0x0E` |
| 输入源 | `0x11` | Netflix | `0x0F` |

右侧音量键经补测确认为 **BLE HID 报文**（`01 0C 00` / `01 0D 00`），不是
IR/CEC 独占，因此在 Windows 侧可捕获、可映射。

- **语音键不产生任何 HID 报文**（纯 ATVV）→ 无需吞键，也不存在 F5 粘键风险。
- 按住不放只产生一次按下/松开，遥控器不自行连发（连发由应用侧手势引擎决定）。

代表性日志：

```text
[+ 23146.9ms] G-COL01 HID report_id=0x01 len=3 usage?=0x0007 b=[01 07 00]   确定按下
[+ 23319.1ms] G-COL01 HID report_id=0x01 len=3 usage?=0x0000 b=[01 00 00]   确定松开
[+ 24781.7ms] G-COL01 HID report_id=0x01 len=3 usage?=0x000B b=[01 0B 00]   返回按下
```

## 结论与边界

- 语音：ATVV 完全兼容，**无需改 `sayall-core`**；不需吞键。
- 按键：需要新增 Google VID/PID 识别、多 HID 集合绑定（Col01/Col02），
  以及新的 3 字节报文解码与按键码表。
- 本记录只证明**协议与输入形态**在真机上符合预期，**不代表**产品集成后的
  端到端功能通过；集成后仍须对 Chromecast Remote 单独完成 Windows 真机验收
  （语音生命周期、全部按键、断连/睡眠恢复、无粘键）。

## 复现

```text
cargo run --release -p sayall-windows --example atvv_probe -- <MAC或名称> 40
cargo run --release -p sayall-windows --example hid_probe -- 72
```

运行前需由用户从托盘正常退出 SayAll。
