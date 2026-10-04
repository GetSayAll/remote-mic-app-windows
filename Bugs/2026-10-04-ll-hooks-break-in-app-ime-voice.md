# 主进程 LL 键盘钩子使本窗口输入法语音热键失效（豆包，2026-10-04 实证）

- 状态：根因已定位（真机证据矩阵完整）；**修复已实施（切片 1–3 完成并提交），
  待 Andy 真机验收**
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

## 修复方向（已定：Andy 2026-10-04 选定"钩子迁出主进程"）

把两个 LL 钩子**迁出主进程**，放入独立"按键宿主"进程（同一 exe 以 `--sayall-key-host`
模式 re-exec；宿主**必须保持普通用户权限**——基础语音路径不得依赖提权或 helper，且
「全按键支持」的 helper 轨保持不变、互不依赖）。宿主进程永不拥有前台顶级窗口，即不触发
上述规则。

**采用「宿主=哑中继 + 主进程决策」结构（尽量少搬逻辑）**：

- 宿主的钩子回调把事件 `(vk, scan, msg, flags, time)` 经本地 IPC 送给主进程的决策泵：
  - 普通事件：异步投递（主进程照旧计数/观察），宿主立即放行；
  - 可能在主进程被吞的事件（F5 0x74、遥控器映射键集、录入模式下的全部键）：同步询问
    （有界等待，超时/主进程缺席一律放行=fail-open）。
- 主进程决策泵复用现有全部逻辑与静态状态（SESSION_ACTIVE、配对表、观察窗口、录入、
  统计、feed_edge、observe_os_visible_down……基本原样保留）。
- 宿主侧只保留机械部分：钩子安装/卸载、链头策略（09-27 实证 FIFO，维持既有"只装一次"）、
  10s bump 计时（抑制器侧现状）、消息泵、IPC 收发、进程生命周期。
- 其余语义不变：60ms 有界等待、DOWN 漏则 UP 必放、排除集、preheld、统计日志。
- 「全按键支持」影响评估：无功能影响——helper（提权、计划任务、报告层）与本修复正交；
  宿主刻意不做提权、不依赖 helper；helper 关闭/未装时基础路径不受影响（现状保持）。
  验收时同时覆盖"helper 开 / helper 关"两种状态。

**实施切片（每片独立提交）**：
1. 宿主模式骨架（`--sayall-key-host` argv 分支、IPC 握手、单实例豁免、退出联动、fail-open）；
   ✅ 提交 `0cbb0e3`；运行验证：宿主启动/握手/退出路径有日志，失败即 fail-open。
2. 抑制器迁移（含 F5 吞键、bump、武装信号）；✅ 提交 `0cb4158`；
   实现说明：宿主只做机械搬运（钩子/消息泵/BUMP/ASK 转发），决策与全部静态状态留在
   主进程 `key_suppressor::handle_host_message`；ASK/VERDICT 有界 200ms，超时放行。
   修复三处实测缺陷（均有回归测试）：OK 与后续命令同段到达被 `BufReader::into_inner`
   丢弃；BYE 与钩子安装赛跑导致 WM_QUIT 无人投递的 join 挂起；**accept 继承监听
   套接字的非阻塞模式**（Windows 语义：新套接字拷贝监听套接字的 FIONBIO——首轮
   验收包实测中主进程把正常连接误判为断开：宿主 `start=passed` 却无任何
   hook_report、后续 `gate_active=0`；修复 = accept 后显式 `set_nonblocking(false)`；
   回归用例覆盖**生产 accept 路径**（寻常单测的阻塞监听器不会暴露该坑），阳性
   对照下必失败——`prod_accept_path_uses_blocking_stream`）。
3. key_gate 迁移（含观察窗口/录入/边沿流）；✅ 提交 `ae76177`；
   实现说明：决策全部保留在 `key_gate::handle_host_event`（在宿主读线程串行执行，
   配对表 thread-local 语义不变）；宿主对每条键盘事件同步 ASK（kind=gate），
   60ms 有界等待在主进程内照旧；门控存活由宿主 `HOOK kind=gate installed=` 报告
   驱动 `GATE_ACTIVE`（fail-open：宿主缺席/断连即全透传）。
   测试语义：`#[cfg(test)]` 下 KeyGate 句柄存活计入 `is_gate_thread_alive()`，
   引擎测试无需宿主进程。
4. 清理诊断开关、按本文件矩阵复验 + `Testing/WindowsOnboardingWizard.md` 用例三。
   进行中：`ime_self_probe` 已移除；`SAYALL_DIAG_*` 开关暂留（验收排障）；
   `src/lib/key-probe.ts` 暂留至验收通过（纯日志、不改变行为），合入 main 前移除。

**验收方法（Andy 真机）**：
1. 安装本分支验收包（`ONBOARDING_WIZARD_ENABLED=true`），进入向导第⑤步；
2. 应用窗口前台时按键盘**右 Alt** 或遥控器**语音键** → 豆包语音条出现 → 说话 →
   松开应上屏且语音条消失（成对清理）；
3. 回归：遥控器已映射按键照常触发映射动作（门控经 IPC 后语义不变）；
   按住说话快捷键（右 Alt 等）成对注入；断连/退出无粘键；
4. helper 开/关两种状态各跑一遍（宿主与 helper 正交，基础路径不依赖 helper）。

## 现场调试开关（本次排查引入，修复提交时清理）

- `SAYALL_DIAG_NO_HOOKS`：`1|both|suppressor|gate|all` 按名关闭对应 LL 钩子（安装前返回）。
- `SAYALL_DIAG_SUPPRESSOR_NO_BUMP`：保留抑制器钩子但停用 10s 链头 bump。
- `examples/doubao_ime_probe.rs`：新增 `watch`（前台窗口 + 活动配置逐秒采样）与
  `cycle-when-foreground`（等目标进程成前台后切走再切回豆包）两个模式；`hkl` 读回。
- 前端 `src/lib/key-probe.ts`：Alt/Ctrl/Shift 类键 DOM 到达日志（`frontend event=key_probe`）。
- 临时诊断页（不入库）：`diag-page/index.html`；经 `--config` 覆盖 `frontendDist` 构建。
