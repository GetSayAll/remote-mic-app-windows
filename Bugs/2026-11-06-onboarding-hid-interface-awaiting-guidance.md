# 2026-11-06 向导②步卡住：遥控器语音已连接，但 Windows 没有按键通道，界面却只说「正在启动」

- 发现日期：2026-11-06（用户反馈，附截图与「复制诊断信息」文本）
- 状态：**界面引导已修（自动化 passed）；用户机恢复与真机复验 deferred**
- 交付边界：本次前端修复**不在已发布的 1.8.3 中**；1.8.3 只包含 2026-11-01 的
  重连修复。用户先升级 1.8.3 是为了消除重连缺陷，本引导需后续构建。
- 影响范围：1.8.0（构建 `f3f5212`，即 v1.8.0 发布提交）、Windows 10 22H2
  build 19045、RC003
- 功能点：首次设置向导②步门禁、Raw Input 监听状态、遥控器普通按键

## 现象

用户停在向导②步「连接遥控器」：

- 已找到并连接「小米蓝牙语音遥控器 2 Pro」，界面显示「已连接」；
- 按键行停在「等待控制按键」，页脚一直写「按键监听正在启动，请稍等一下。」；
- 「继续」不可用，无论按多少次普通按键都没有变化。

## 证据（用户提供的诊断块）

```text
App version: 1.8.0
Build: f3f52127973ef0327957c3058d4b5f3413ad803e
Windows version: 10.0.19045
Remote: phase=ready model=rc003 voice_ready=yes reconnects=0 error=no button_observed=no
Raw input: phase=awaiting matched=0 raw_events=0 semantic_edges=0 stale_events=0
Input tool: unset hotkey=left_control+left_windows full_key_support=off capture=off
Buttons observed: 0/3
Gate: remote.listener_not_ready
```

## 根因

### 直接原因（已由诊断块确认）

`Raw input: phase=awaiting matched=0 raw_events=0` 表示 Raw Input 监听器已经运行，
但系统里没有匹配这支遥控器的 HID 接口。普通按键只通过这条链路进入应用，
因此 `raw_events` 与 `semantic_edges` 都不会增长，②步不可能放行。

这与 2026-10-22 的②步死锁不同：那次 `semantic_edges` 在增长、按键事实上已经到达
（RC003「全按键支持」经助手桥接），只是门禁错误地要求 Raw Input 就绪。本次
`full_key_support=off`、`semantic_edges=0`，是按键根本没有到达应用。

### 为什么 Windows 没有 HID 接口（未完全确认）

只有一份诊断块、没有用户机日志，不能区分「配对缓存里没有 HID 服务节点」与
「BLE HID 链路楔死」。已知事实：

- `Remote: ... reconnects=0`：这个进程只建立过一次 BLE 会话，没有走断链重连。
  因此本次不是 1.8.0 断链清理拆链路那条路径的直接结果；HID 接口在这个会话
  建立时就已经不在。
- 同一构建、同一 Windows 版本在 2026-11-01 的用户日志里出现过相同形态：
  `matched_device_count=0`，遥控器 HID 接口在整个失败时段没有出现
  （`Bugs/2026-11-01-ble-reconnect-cleanup-and-model-probe.md`）。但那次伴随
  大量重连失败，和本次不同。
- 2026-09-16 同一台机器的 BLE 楔死态只有完整重启能解除；那次也观察到
  `awaiting_remote_hid_interface=true`。

因此本次不把「升级后一定恢复」写成结论；重新配对、完整重启是更直接的
恢复步骤，升级到 1.8.3 只用于顺带消除已知的重连缺陷与获取新引导。

### 产品缺陷（本次修复）

`evaluateRemote` 把两种完全不同的状态都归到 `remote.listener_not_ready`：

- `starting`：监听线程刚启动，等一下就会就绪；
- `awaiting`：监听器已在跑，但系统没有 HID 接口，再等也不会有按键。

界面统一写「按键监听正在启动，请稍等一下」，用户没有任何可执行动作，形成
无声死等。

## 修复

- `src/onboarding/flow.ts`：新增阻断码 `remote.hid_interface_missing`；
  已连接、未观察到按键且 `rawInputPhase === "awaiting"` 时返回该码，
  `starting` / `failed` 仍返回 `remote.listener_not_ready`。
- `src/pages/OnboardingPage.vue`：新增页脚提示——「Windows 还没有识别到这支遥控器的
  按键通道。打开蓝牙设置重新配对。」
- `src/onboarding/steps/RemoteStep.vue`：
  - 已连接且 `awaiting` 时，按键行改为「先按一下普通按键；仍无反应时，按下面的
    步骤重新配对。」，避免只重复页脚的状态句；
  - 新增修复卡「没有识别到遥控器的按键通道」，给出「打开蓝牙设置」入口与
    完整步骤：删除这支遥控器 → 按「TV」键 3 秒重新配对 → 仍不行用「重启」
    （不要用「关机」再开机，快速启动不会完整复位蓝牙）。
  - 动作按钮放在长说明**之前**：1066×800 最小窗口下内容区只露出卡片上半部分，
    按钮必须先可见，不需要用户先滚动。
- `src/lib/i18n/en/onboarding.ts`：补齐上述文案的英文翻译。

## 验证

- `vitest run src/lib/i18n.test.ts src/onboarding/flow.test.ts src/pages/OnboardingPage.test.ts`
  **passed**：115 passed。
  - 新增门禁用例：`awaiting` + 未观察按键 → `remote.hid_interface_missing`；
    `starting` 仍为 `remote.listener_not_ready`；已观察按键时任何 Raw Input
    阶段都放行（保留 2026-10-22 的修复）。
  - 新增界面用例：`awaiting` 时出现修复卡与「打开蓝牙设置」，且不再出现
    「按键监听正在启动」；`starting` 仍显示启动提示，不提前给重新配对卡。
- 全前端 `vitest run` **passed**：PR 基线（`153b239`）626 passed / 10 skipped；
  改动分支 629 passed / 10 skipped。
- `vue-tsc --noEmit` **passed**。
- `vite build` **passed**（150 modules）。
- `cargo` 未改动。

### 浏览器布局取证（2026-11-06）

- 方式：Vite 开发页 + 临时桥接快照（`connection=ready` / `rc003` /
  `rawInput.phase=awaiting`），**不是真机**；截图与量测存
  `artifacts/local-test/2026-11-06-onboarding-hid-channel/`。
- 1066×800（最小窗口）：修复卡按钮调整前位于内容区下方（内容底 704，
  按钮 690–727，需滚动）；调整后按钮 607–643，完整可见。页脚提示
  `scrollWidth=453 / clientWidth=453`，单行不溢出；「继续」752–782 可见；
  无水平溢出。
- 1280×800：按钮 580–617，完整可见；页脚提示同样 453/453；
  「继续」752–782 可见；无水平溢出。
- 证据：`onboarding-awaiting-1066x800.png`、`onboarding-awaiting-1280x800.png`、
  `layout-metrics.json`。
- 边界：浏览器预览没有真实 WebView2/缩放/系统字体差异；安装包内的最终目视
  仍 `deferred`。

## 仍未覆盖（deferred）

- 用户机升级到 1.8.3 后的真机结果：需确认重新配对或完整重启后
  `Raw input` 从 `awaiting matched=0` 变为 `ready matched=1`，并能通过②步。
- 若升级后仍为 `matched=0`，需要用户提供应用日志与
  `binding_initial` / `binding_audit` 记录，才能区分「HID 服务节点缺失」与
  「路径存在但 VID/PID 不匹配」。本次没有凭猜测扩大 Raw Input 匹配范围。
- RC001 / RC003 真机走查均未执行；本次只有前端逻辑与组件测试。

## 隐私检查

未包含蓝牙地址、HID 路径、设备实例 ID、用户名、语音内容或个人路径。
