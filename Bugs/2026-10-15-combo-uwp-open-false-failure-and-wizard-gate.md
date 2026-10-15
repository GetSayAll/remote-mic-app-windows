# 组合动作第二轮现场：UWP 应用"已打开却判失败" + 向导门禁漏洞（名称为空可越过）

- 状态：已修复（自动化 + 真机探针 passed；整链路真机复验 deferred——需重装含本修复的包）
- 影响范围：组合动作「打开应用 / 等待应用」步骤（UWP 目标）；组合动作向导门禁与错误文案
- 报告：2026-10-15 用户现场（第二轮）

## 现象

1. 组合动作里「打开计算器」：测试动作显示「执行失败，后续步骤已停止」，**实际计算器已经打开**。
2. 组合动作名称没填也能进入下一步；随后测试报「测试失败：invalid:name_empty」。

## 现场证据

诊断日志（同一台机器、同一构建 `33280c03`）：

```
map_macro_step … step=0 action=open_url        terminal_result=passed elapsed_ms=103
map_macro_step … step=1 action=open_application terminal_result=failed elapsed_ms=8165   ← 等满 8 秒超时
map_macro_terminal … terminal_result=step_failed
```

对照：同一动作里记事本那步只用了 151ms（应用已在运行，判据立刻命中）。

真机探针（本仓库 `crates/sayall-windows/examples/app_readiness_probe.rs`，计算器当时正在运行）：

```
calc:     known=true running_process=true  visible_window=false window_presence=false
notepad:  known=true running_process=false visible_window=false window_presence=false
explorer: known=true running_process=true  visible_window=false window_presence=true
```

`Get-Process CalculatorApp` 有 3 个进程、`MainWindowTitle` 全空；窗口实际属于
`ApplicationFrameHost.exe`。

## 根因（两处独立）

1. **UWP / 打包应用的窗口不归自己**：`calc.exe` 只是启动器，真正的
   `CalculatorApp.exe` 进程没有归属自己的可见窗口（窗口属于
   `ApplicationFrameHost.exe`）。「打开应用」的就绪判据当时只有"按进程匹配窗口"
   一路 ⇒ 对 UWP 目标**永远**等不到 ⇒ 8 秒超时 ⇒ 判失败并中止整条动作，
   而应用其实已经打开。
2. **向导门禁漏洞**：基本设置里点动作卡会自动跳到第②步（绕过"名称为空不给
   下一步"的禁用逻辑），步骤指示条也可直接点击跳步；`addStep` 当时还是累加语义。
   于是空名称草稿能走到测试/保存，后端按校验拒绝并把稳定 token
   （`invalid:name_empty`）直接显示给用户。

## 修复

- 就绪判据新增 `running_process`（Toolhelp 进程快照，按映像名/路径/AUMID 匹配）：
  - 「打开应用」超时后**若进程在跑** → `Unknown`（"请求已发送，请在目标 App 确认
    结果"），后续步骤继续，整宏终态 `completed_with_unknown_result`；
    进程也不在才判 `Failed`（纯函数 `open_application_timeout_outcome` + 单测）。
  - 「等待应用 → App 已打开」同样改用进程判据（UWP 可判定）。
  - 日志 `map_macro_step_detail … readiness=process_running_window_not_observed`。
- 向导（对齐 Mac）：基本设置里选动作**不再自动跳步**、改为替换语义；步骤指示条与
  "下一步"统一走 `goToStep` 门禁（名称为空 → 「先填写组合动作名称」）；保存前兜底。
- 前端 `macroStartErrorLabel`：`already_running / not_found / invalid:*` 映射成
  用户语言（"请先填写组合动作名称" 等），不再显示机器码。

## 验证

- 探针真机读数（上表）：calc 三路判据与修复假设逐项吻合；explorer 为阳性对照
  （Win32 应用窗口可见）；notepad 未运行 → 两路皆 false（判据不撒谎）。
- Rust：`cargo test --workspace` 全绿（core 54 / windows 377 含新单测 / app 117）、
  `cargo fmt --check`。
- 前端：vitest 392 passed | 11 skipped（新增：空名称门禁 1 例、测试失败中文文案 1 例、
  新建流程改写 1 例）、`vue-tsc`。

## 边界与遗留

- 「等待应用 → **App 窗口已出现**」对纯 UWP 目标仍会超时失败（窗口不属于目标进程，
  需按 AUMID 匹配窗口才能补强）——记入 TODO「组合动作」条目，暂未实现。
- 真机复验 deferred：重装后重测「打开计算器」应显示"无法确认第三方 App 的结果"
  且后续步骤继续执行；向导空名称应被拦在第一步。
