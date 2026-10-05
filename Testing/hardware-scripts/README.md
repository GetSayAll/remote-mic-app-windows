# 硬件信号脚本（Testing/hardware-scripts）

本目录是「模拟硬件信号跑 onboarding / 按键与语音链路」所用的**信号脚本**，全部是纯 JSON，
本仓库自带、可直接手写，**不依赖任何私有仓库**。

- `hid-buttons-smoke.json`：最小冒烟（连接 → HID 挂载 → 3 个按键的按下/释放）。
- `rc003-onboarding-walkthrough.json`：走查首次设置向导 ①–⑦ 用（周期按键 + 一次 ATVV 语音会话）。
- `hid-button-app-script.json` / `rc001-short-voice-app-script.json`：两份**样例导出**（数据契约样本，
  用于 `crates/sayall-windows/src/hardware_script.rs` 的契约测试）。它们与私有模拟器
  `GetSayAll/hardware-simulation` 的 `hardware-sim export-app-script` 输出同格式；需要别的场景时，
  复制样例手写即可——**不是构建/测试依赖**。

## 格式

```json
{
  "schemaVersion": 1,
  "id": "任意标识",
  "sourceRevision": null,
  "events": [
    { "atMilliseconds": 0,   "kind": "ble_connected" },
    { "atMilliseconds": 50,  "kind": "hid_attached" },
    { "atMilliseconds": 3000, "kind": "hid_report", "reportID": 1, "dataHex": "4a0000000000" },
    { "atMilliseconds": 3120, "kind": "hid_report", "reportID": 1, "dataHex": "000000000000" },
    { "atMilliseconds": 45000, "kind": "voice_control", "dataHex": "0b010002030078" },
    { "atMilliseconds": 45500, "kind": "voice_control", "dataHex": "04030201" },
    { "atMilliseconds": 46000, "kind": "voice_audio", "dataHex": "11…（40 字节）" },
    { "atMilliseconds": 46500, "kind": "voice_audio", "dataHex": "11…（80 字节）" },
    { "atMilliseconds": 48000, "kind": "voice_control", "dataHex": "00" }
  ]
}
```

| kind | 字段 | 语义 |
|---|---|---|
| `ble_connected` / `ble_disconnected` | — | 遥控器连接建立 / 断开（连接建立即把连接相位置 ready） |
| `hid_attached` / `hid_removed` | — | Raw Input 设备挂载 / 移除（移除会释放所有按下状态） |
| `hid_report` | `reportID?`, `dataHex` | 原始 HID input report，走生产 `decode_report_usages` + `ButtonStateMerger` |
| `voice_control` | `dataHex` | ATVV 控制通道字节（能力包、`STREAM_START`/`STREAM_STOP`） |
| `voice_audio` | `dataHex` | ATVV 音频通道字节（IMA-ADPCM，120 字节 = 240 采样） |
| `gatt_value` | `characteristicUUID`, `dataHex` | 其他 GATT 值；当前应用只记日志 |
| `raw` | `transport`, `originalKind?`, `payload` | 未识别事件的兜底，保留原文 |

## 注意

- 事件按 `atMilliseconds` 升序回放；同刻事件保持文件顺序。
- 按键门禁只在对应步骤计数（第②/⑥ 步）：**按键要覆盖操作者停留在那一步的时间窗**（走查脚本按周期重复）。
- 音频必须凑满整帧（120 字节）；不足一帧会解出 0 采样，第⑤步报「没有音频数据」。
- 文件保存为 UTF-8 **无 BOM**（应用解析器容忍 BOM，但其他工具如 `jq` 可能不接受）。
- 运行方式见 `../run-hardware-signal-script.ps1` 与 `../../Testing/WindowsOnboardingWizard.md`
  「模拟硬件信号（无真机跑向导）」。
