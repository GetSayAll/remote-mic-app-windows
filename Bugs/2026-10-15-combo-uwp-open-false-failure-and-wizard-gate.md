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

## 第三轮（2026-10-15 晚，用户复测）

**现象**：多次执行「打开计算器」会打开多个窗口；测试结果显示「请求已发送，请在目标
App 确认结果」而不是「已完成」；结果出来得慢（约 8 秒）。

**证据（本机实测）**：

```
# calc.exe 或 shell:AppsFolder\… 每启动一次就多一个实例
before: 2  → Start-Process calc → 3   （用户看到"多个窗口"的机制）
before: 3  → AUMID 激活 → 4 → 5      （AUMID 启动同样新开实例）

# 判据口径对照（修复前代码）
calc:                                          visible_window=false   ← 按映像名看不到窗口
shell:AppsFolder\Microsoft.WindowsCalculator…: visible_window=true    ← 只有 AUMID 口径看得见
```

**根因**：打包应用（计算器）的窗口只有按 **AUMID** 才能枚举到；"已运行"识别不到 ⇒
每次执行都走"启动"分支 ⇒ 新开窗口；同时就绪判据（只有映像名口径）等不到窗口 ⇒
耗满 8 秒超时 ⇒ 报"结果不可确认"。三个现象同源。

**修复**：预设应用新增 AUMID 表（`preset_aumid`，只登记本机实测确认项：计算器）；
`target_window_visibility` 合并「映像名 + AUMID」两个口径，`application_readiness`
与激活路径共用；窗口枚举新增 `largest_visible`；打包应用**已有可见窗口时只前置、
绝不重新启动**（`activate_known_window`，失败即报"Windows 拒绝切换到前台"）。

**真机验证（修复后，同一台机器）**：

```
calc: known=true running_process=true visible_window=true window_presence=true   ← 首轮命中，不再等超时
activate calc: ok=true
激活前进程数 5 → 激活后进程数 5（无新实例；修复前每执行一次 +1）
```

**边界**：AUMID 表按应用逐个登记（未登记的应用维持原路径）；"等待应用 → 窗口已出现"
对未登记 AUMID 的打包应用仍不可判定。
## 第四轮（2026-10-15 晚，按键映射「打开计算器」首次不置前）

**现象**（用户现场）：按键映射绑定"打开应用 → 计算器"。**第一按**：计算器打开了
（只开一个，第三轮修复生效），但**没有到最前台**；**再按一次**才置前。

**复现与证据（本机，冷启动 = 无 CalculatorApp 进程时激活一次）**：

```
activate calc: ok=true
foreground_pid=19972      ← 不是计算器；窗口起来了但前台仍是别的进程
calc 进程: 21700
```

**根因**：ShellExecute 启动打包应用（计算器）后，新开的实例**不会**自动取得前台
（我们的进程是后台进程，调度者不是前台进程）；第三轮新增的"已运行 → 前置"只在
**窗口已存在**时才生效，所以表现为"第一按只打开、第二按才置前"。

**修复**：`activate_or_launch` 预设分支的两条启动路径（解析启动目标 / `launch_new`）
在启动成功后统一走 `foreground_after_launch`：按本应用窗口口径（含 AUMID 合并）
轮询窗口出现（10 × 150 ms 有界），出现即 `show_and_force_foreground` 强制置前
（含既有前台锁解锁机制）；预算内没等到窗口则记 `skipped` 返回 Ok（应用可能仍在启动）。

**真机验证（修复后）**：

```
# 场景 1：冷启动（关闭全部计算器后第一次按）
前台: PID=12248 sayall-windows-app → activate calc →
前台: PID=31396 ApplicationFrameHost（标题「计算器」）    ← 一次到位
calc 进程数: 1                                            ← 不多开

# 场景 2：被切到后台后（先激活记事本占前台）
前台: Notepad → activate calc →
前台: ApplicationFrameHost（计算器）；calc 进程数仍为 1    ← 只前置、不新开
```

**备注**：计算器的窗口（含标题）由 `ApplicationFrameHost.exe` 持有——这也是"按
进程/映像名看不到它"的同一件事；判据与置前都必须走 AUMID/窗口口径。