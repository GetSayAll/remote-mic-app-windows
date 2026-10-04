# 主进程 LL 键盘钩子使本窗口输入法语音热键失效（豆包，2026-10-04 实证）

- 状态：根因已定位（真机证据矩阵完整）；修复未实施
- 影响：无线麦「按住说话验证」（向导第⑤步）在**自己的 WebView2 输入框**里无法通过；
  遥控器语音键 → 文字链路在应用窗口内不可用（键盘右 Alt / 遥控器合成右 Alt 均不触发豆包语音条）
- 范围：RC003 + 豆包输入法（0.8.x）；`crates/sayall-windows/src/key_suppressor.rs`（F5 抑制器）
  与 `key_gate.rs`（观察窗口/吞键门控）两个 WH_KEYBOARD_LL 钩子的安装位置

## 证据矩阵（同一台机器、同一时段、同一输入法会话「中 豆包」）

| LL 钩子所在进程 | 前台根窗口所属进程 | 页面 | 右 Alt → 豆包语音条 |
| --- | --- | --- | --- |
| 应用（抑制器 + gate 都在） | 应用 | 正常/诊断页 | ❌ |
| 应用（仅 key_gate） | 应用 | 诊断页 | ❌ |
| 应用（仅抑制器） | 应用 | 诊断页 | ❌ |
| 无钩子 | 应用 | 诊断页 | ✅ |
| 应用（两钩子都在） | AltProbe（独立最小 wry 宿主） | 探针页 | ✅ |
| altprobe 进程（全透传钩子） | 应用 | 诊断页 | ✅ |

**规则：当且仅当"前台根窗口的进程 == LL 键盘钩子所在进程"时失效。**
钩子在他进程、或本窗口不是前台根时均正常。

## 已排除项（均有对照实测）

- 会话输入法状态：任务栏指示器两窗口一致（「中 豆包」）；应用内探针读回 `active_is_target=true`；
  "切走再切回豆包"的会话级循环无效。
- 键投递：应用窗口 DOM 收到完整 AltRight DOWN/UP（与探针窗口事件同型）。
- WebView2 宿主配置：命令行参数、user-data-dir 结构、`AreBrowserAcceleratorKeysEnabled` 均一致；
  全新 profile 无效；最小 wry 宿主（同版本 153.0.4234.48）稳定可用。
- 页面内容：正常前端与静态诊断页均复现（钩子在主进程时）。
- 焦点/时序：先打字证明输入法焦点后立即按键，仍复现；长按 6 秒（含系统连发）也不出现语音条。
- 现场对照：Chrome / 记事本 / Edge 同一按键正常。

## 修复方向（待实施）

把两个 LL 钩子**迁出主进程**，放入独立"按键宿主"进程（同一 exe 以 `--key-hook-host`
模式运行，或专用小进程）。宿主进程永不拥有前台顶级窗口，即不触发上述规则。

- 上游（主进程 → 宿主）：会话武装（`set_session_active`）、链路守卫、Raw Input 归因武装窗口、
  观察窗口 begin/end（含排除集）、快捷键录入 begin/end（含 preheld）、遥控器在线状态、退出。
- 下游（宿主 → 主进程）：吞/放边沿流（按钮语义边沿）、观察计数、录入边沿、
  抑制器统计、遥控器 HID 活动通知。
- 机制：本地 IPC（loopback TCP 或命名管道）；协议带版本号与 generation；宿主崩溃/未启动时
  fail-open（等同现状"钩子安装失败"路径：记录、降级、不阻塞语音）。
- 必须保持的现有语义：F5 吞键、60ms 有界等待、链头策略（09-27 实证为 FIFO，勿再"抢链头"）、
  DOWN 漏则 UP 必放（防粘键）、观察窗口的排除集、快捷键录入的 preheld 处理。
- 验收：修复后重复本表全部六行实验；再做 `Testing/WindowsOnboardingWizard.md` 用例三。

## 现场调试开关（本次排查引入，修复提交时清理）

- `SAYALL_DIAG_NO_HOOKS`：`1|both|suppressor|gate|all` 按名关闭对应 LL 钩子（安装前返回）。
- `SAYALL_DIAG_SUPPRESSOR_NO_BUMP`：保留抑制器钩子但停用 10s 链头 bump。
- `examples/doubao_ime_probe.rs`：新增 `watch`（前台窗口 + 活动配置逐秒采样）与
  `cycle-when-foreground`（等目标进程成前台后切走再切回豆包）两个模式；`hkl` 读回。
- 前端 `src/lib/key-probe.ts`：Alt/Ctrl/Shift 类键 DOM 到达日志（`frontend event=key_probe`）。
- 临时诊断页（不入库）：`diag-page/index.html`；经 `--config` 覆盖 `frontendDist` 构建。
