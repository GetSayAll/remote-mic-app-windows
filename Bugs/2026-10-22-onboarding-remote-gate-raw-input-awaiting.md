# 2026-10-22 向导②步死锁：已收到遥控器按键，却因 Raw Input 停在 awaiting 无法继续

- 发现日期：2026-10-22
- 状态：已修复（自动化 passed；真机复验 deferred）
- 影响范围：1.0.0（构建 `a4c1311d405509daf486fff3c82f7c8ec76f03fe`）、Windows 10
  19045、RC003 + 「全按键支持」开启；任何按键经增强捕获助手送达、但 Raw Input
  没有绑定到 HID 接口的机器
- 功能点：向导②步门禁、Raw Input 监听状态、RC003 全按键支持
- 现象：界面两项检查都已打勾（「已收到遥控器按键」「控制按键已收到」），页脚仍显示
  「按键监听正在启动，请稍等一下。」，「继续」不可用，向导永久停在 2/7。
- 复现条件：连接 RC003，开启「全按键支持」，使普通按键经助手桥接进入映射引擎；
  同时 Raw Input 未绑定到遥控器 HID 接口（`matched=0`）。按任意普通键即可复现。
- 正常预期：收到至少一次真实按键边沿后，②步应放行；监听器状态只用于“尚未收到
  按键”时的等待提示，不能覆盖已经发生的事实。
- 证据：用户提供的诊断块（无日志）：
  - `Gate: remote.listener_not_ready`
  - `Remote: phase=ready model=rc003 voice_ready=yes ... button_observed=yes`
  - `Raw input: phase=awaiting matched=0 raw_events=0 semantic_edges=198`
  诊断块里的 `Buttons observed: 0/3` 属于第⑦步计数，不是②步的判据。
- 根因：`evaluateRemote` 先检查 `rawInputPhase === "ready"`，再检查
  `remoteButtonObserved`。RC003「全按键支持」的边沿由提权助手经
  `EngineMessage::GateEdge` 直接送入映射引擎，不依赖 Raw Input 绑定；因此
  `semantic_edges` 在增长、界面能打勾，而 `rawInputPhase` 可以长期是 `awaiting`。
  门禁把内部通道状态当成继续的前置条件，形成死锁。
- 仍未知边界：该机 Raw Input 为什么没有枚举到 HID 接口（`matched=0`）——只有一份
  诊断块、没有本机日志，无法判定是接口未出现、目标地址过滤还是系统枚举差异。
  本次修复不依赖该结论；若后续关闭「全按键支持」后按键失效，应另开一条 Bug，
  按 `Testing/WindowsOnboardingWizard.md` 收集 `raw_input binding_initial` /
  `binding_audit` 日志后再判定。
- 修复：`src/onboarding/flow.ts` 的 `evaluateRemote` 改为“已收到按键即放行”，
  未收到按键时才按 Raw Input 状态提示等待。测试补三种非 ready 阶段
  （starting / awaiting / failed）与“已观察按键”的组合，并把原阻断用例显式设为
  未观察到按键。
- 验证：
  - `vitest run`：455 passed | 10 skipped（`passed`）
  - `vue-tsc --noEmit`：`passed`
  - `vite build`：`passed`
  - `scripts/ci-preflight.ps1`：7/7 步通过（前端 455 passed | 10 skipped、Rust
    workspace 测试与 `cargo check`、runtime-simulation 编译检查均 `passed`）
  - 现场那台 Windows 10 + RC003 + 全按键支持机器的真机复验：`deferred`
- 隐私检查：未包含蓝牙地址、HID 路径、设备身份、语音内容或个人路径。
