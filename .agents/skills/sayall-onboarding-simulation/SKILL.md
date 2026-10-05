---
name: sayall-onboarding-simulation
description: 在 SayAll Windows 仓库用「模拟硬件信号」跑首次设置向导（onboarding ①–⑦）与按键、语音链路的回归：构建 runtime-simulation 构建、用信号脚本驱动、隔离状态目录、分步核验与取证。没有物理遥控器、或要给 PR 出「向导全流程通过」证据时用。触发词：onboarding 模拟测试、模拟硬件信号、信号脚本、hardware-simulation、没有遥控器怎么测向导、simulation 构建、run-hardware-signal-script、走查向导、voice_attempt。
---

# SayAll Windows：用模拟硬件信号跑 onboarding

## 何时用

- 手上没有物理遥控器 / 不想每次都真人操作，但要回归首次设置向导与按键、语音链路；
- 复现向导某一步的卡点（② 按键门禁、③ 端点、⑤ 语音会话、⑥ 按键检查）；
- 给 PR 出「向导 ①–⑦ 全流程通过」的证据。

## 边界（先读）

- **模拟 ≠ 真机**：配对、射频、权限、驱动、真实音质、安装包行为仍需真机；`Testing/WindowsOnboardingWizard.md` 的用例一~五仍要按 RC001/RC003 分别做。
- 第⑤步的**转写文字**在模拟轨道靠驱动写入输入框；真机必须真人说话。
- 本仓库**不依赖任何私有仓库**：信号脚本是纯 JSON，格式在本仓库 `Testing/hardware-scripts/README.md`（可手写）。私有模拟器 `GetSayAll/hardware-simulation` 能导出同格式脚本，属**可选**参考，不是构建/测试依赖。

## 一、准备（每台机器一次）

```powershell
cargo build -p sayall-windows-app --features runtime-simulation   # 仿真二进制（debug 即可）
pnpm dev                                                          # 仿真二进制 devUrl = localhost:2430
```

- `cargo build` 的产物是 **dev 模式**：打开的是 `devUrl`，所以必须先起 `pnpm dev`；只有 `pnpm tauri build` 轨道才内嵌前端。
- 单实例互斥：跑之前先退出已运行的应用（`run-hardware-signal-script.ps1 -StopExisting` 走优雅退出事件 `Local\SayAll-GracefulExit`）。
- 构建报 `拒绝访问 (os error 5)`：应用还在跑、锁住 exe——先优雅退出再 build。
- 排查"界面没起来"：用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`，再查 `http://127.0.0.1:9222/json` 看页面真实 URL。

## 二、跑（最快路径）

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\run-hardware-signal-script.ps1 `
    -ScriptPath Testing\hardware-scripts\rc003-onboarding-walkthrough.json -StopExisting
```

- 脚本内部设置：`SAYALL_WINDOWS_RUNTIME_SIMULATION=1`、`SAYALL_HARDWARE_SIGNAL_SCRIPT=<脚本>`、`SAYALL_RUNTIME_SIMULATION_STATE_DIR=<隔离目录>`。
- 隔离状态目录让向导从第①步开始，**不动真实用户状态**；日志仍在 `%LOCALAPPDATA%\SayAll\logs\sayall-diagnostic.log`。
- 回放日志逐条：`hardware_script action=apply ... at_ms=<n> kind=<kind> terminal_result=passed|failed detail=`。
- 走查脚本内建周期按键：②/⑥ 的门禁**只在对应步骤计数**，按键必须覆盖操作者停留在那一步的时间窗。

## 三、驱动界面（没有人坐在旁边时）

仿真 + dev 模式下用 CDP 驱动（`--remote-debugging-port=9222`，`Runtime.evaluate`）：

- 步骤标题：`document.querySelector('h1').textContent`
- 门禁：底部主按钮（`继续` / `开始使用`）的 `disabled`
- ② 门禁提示、⑤ 状态行 `.onboarding-status-line`、⑤ 成功卡 `.onboarding-card.success`
- ⑤ 转写：`box.value='…'; box.dispatchEvent(new Event('input',{bubbles:true}))`（"物理键盘输入"用独立计数判，脚本注入不会误判为手动输入）
- 完整驱动示例：`drive-onboarding.ps1` 形态的脚本——按标题等到下一步、等门禁 enabled 再点、逐屏 `Page.captureScreenshot` 存证。

## 四、分步通过判据与常见卡点

| 步骤 | 通过判据 | 常见卡点 |
|---|---|---|
| ① 欢迎 | 继续可用 | — |
| ② 连接遥控器 | 门禁码 `remote.button_not_ready` 消失 | 按键事件没落在②停留窗口；缺 `ble_connected` |
| ③ 选择语音设备 | 端点被判为"推荐" | 仿真端点名必须含 `VB-Audio Virtual Cable`（推荐判定按名字），否则永远进不去 |
| ④ 输入工具 | 选完工具后放行 | 豆包要 UAC 授权；模拟轨道建议选微信输入法 |
| ⑤ 按住说话 | `voice_attempt result=passed detail=d240_s240_q0_drain1` | 音频分片不足 120 字节 → 0 采样（提示"没有音频数据"）；会话要覆盖轮询窗口 |
| ⑥ 普通按键 | 3 个**不同**按键 | 按键事件要覆盖到达该步之后的时间 |
| ⑦ 设置完成 | `completed reason=wizard_finished` | — |

## 五、写新脚本

- 契约与示例：`Testing/hardware-scripts/README.md`；最小冒烟用 `hid-buttons-smoke.json`。
- 事件 kind：`ble_connected | ble_disconnected | hid_attached | hid_removed | hid_report | voice_control | voice_audio | gatt_value | raw`（内部标记 `kind`）。
- hex 载荷必须偶数长度；`hid_report` 支持 6/7/9 字节形态（解析见 `crates/sayall-windows/src/raw_input.rs::decode_report_usages`）。
- 按键 usage 表：`ENHANCED_CAPTURE_BUTTON_USAGES`（Back 0x00F1 / Ok 0x0028 / Home 0x004A / Up 0x0052 / Down 0x0051 / Menu 0x0065 / …）；6 字节报告里是 3 个 u16 小端 usage。
- ATVV 音频按整帧给：16 kHz / IMA-ADPCM，**120 字节 = 240 采样**（如 40+80 两次分片）。
- 保存成 UTF-8 **无 BOM**（应用解析器容忍 BOM，其他工具不一定）。

## 六、取证

- 截图 + 日志摘录放 `artifacts/local-test/<日期>-onboarding-simulation/`。
- 必留日志行：`hardware_script action=apply`（回放生效）、`frontend event=onboarding phase=voice_attempt`（⑤ 终态）、`phase=completed reason=wizard_finished`（收尾）。

## 七、坑（都踩过）

1. **PowerShell 5.1 中文脚本必须 UTF-8 带 BOM**：无 BOM 会被按 GBK 解码，字符串被吞、甚至语法错误。
2. **别在应用运行时 `cargo build`**：exe 被锁 → `拒绝访问 (os error 5)`；先优雅退出。
3. **dev 模式需要 dev server**：忘了 `pnpm dev` 只会看到 `localhost 拒绝连接` 的 WebView 错误页。
4. **验证要看生产链路证据**：`hardware_script action=apply` 的 `detail=edges=N` / `decoded_samples=N`，不是 UI 变色。
5. **手写 hex 容易数错长度**：120 字节整帧才是 240 采样；不足一帧解出 0。
6. **仿真状态目录只隔离 settings.json 与 onboarding.json**（两者同目录）；其他状态与日志仍走真实路径。
