# Windows 常用语（会员能力）实施进度与剩余工作（2026-10-15）

> 分支：公开仓 `feat/common-phrases`（worktree `.worktree/common-phrases`）；
> 私有仓 `codex/windows-phrases`（worktree `.worktree/windows-phrases`，包 `packages/windows-common-phrases/`）。
> 产品规则来源（私有仓）：`docs/product/payment.md`（2026-10-12 节）、
> `docs/product/plus-commercial-rules-20261012.md`（G 节）、macOS 参照实现 `packages/macos-common-phrases/`。

## 边界（不可违反）

- 常用语是**会员能力**（Plus/Pro 可用、Free 不可用）；实现、内置资源与测试在私有包。
- 公开仓只保留：稳定动作 ID、按键事件转发、可选适配接口、空壳 crate 与空壳 UI 导出。
  **缺私有组件时**：能力不可用、按键映射页隐藏入口、稳定按键与语音不受影响；不得复制私有实现补齐。
- 常用语正文不进日志；剪贴板事务必须原位快照与恢复（用户期间自行复制不得回滚）。
- 按键认领必须发生在**手势识别之前**、只基于运行时状态，不得写成持久绑定。

## 已完成（含验证证据）

| 提交 | 内容 | 验证 |
| --- | --- | --- |
| 公开 `1a9f8c2` | 接缝骨架：动作 ID、`CommonPhraseHost`+空壳 crate、手势前认领、`key_gate` 面板遮蔽、`focused_input_available` | `cargo test -p sayall-windows --lib` 354 passed |
| 公开 `6d13543` | `common-phrases-panel.html` 第二入口、公开空壳 UI、bridge 三个调用 | `corepack pnpm build` |
| 公开 `920415d` | `CommonPhrasePanelEntry` + `panel_entries`/`select` | 同上 |
| 公开 `874df85` | 进程级组件槽位 `set/current_common_phrase_host` | 354 passed |
| 公开 `d036f4d` | 浮层窗口宿主、`install(app)`、三个命令、capability、依赖 | `cargo check -p sayall-windows-app --features runtime-simulation` exit=0 |
| 公开 `9650011` | `.focusable(false)`（NOACTIVATE 抗 tao 重写）、光标屏居中、`fg_before/after` 判据、`panel-window.css` 透明 | 同上 + `pnpm build` |
| 公开 `869d5e2` | `KeyGate::start()` 与 `report_host_disconnected` 清零面板遮蔽 | 354 passed |
| 公开 `de6cbc2` | 遥控器离线时遮蔽失效（fail-open 兜底） | 354 passed |
| 私有 `db94dc6` | 私有 crate：内置内容+生成器、归档、认领、OLE 剪贴板事务、UIA 校验 + Ctrl+V、宿主接缝、注入脚本 | 22 例 |
| 私有 `6e823f8` | 圆盘面板 Vue + 面板数据/点选 | 注入后 `pnpm build` 通过 |
| 私有 `7dafe52` | 门禁桩真机开关 `SAYALL_COMMON_PHRASES_ASSUME_MEMBER` | 23 例 |
| 私有 `936abc8` | **B1 修复**：选中先快照目标再关面板 + 回归用例 | 23 passed |
| 公开 `363f5ec` | **批次 2**：来源失效 `reset` 调用点、认领边沿配对自持（M2）、退出路径 `shutdown()` 接线（trait 默认方法 + `shutdown_platform_for_exit`） | `cargo test --workspace`（sayall-windows 359 passed，+5 条新测试）；`cargo check -p sayall-windows-app --features runtime-simulation`；`scripts/ci-preflight.ps1` 7 步通过 |
| 私有 `a5f6aed` | **批次 2**：`WindowsCommonPhraseHost::shutdown` 等完在途插入（含 250 ms 恢复窗）；`spawn_with_factory` 测试钩子 | 24 passed（+1 条新用例） |

## 剩余工作（按批，含落点）

> 2026-10-15 晚更新：批次 2 已完成并提交（见上表）。剩余从批次 3 开始；
> 真机走查清单（文末）待出包后执行。

### 批次 3（契约与失败反馈）

4. **公开构建映射页兼容**（评审 M1，会让映射页崩）：`src/lib/bridge.ts:187-194` 补
   `open_common_phrases` union 成员 + `actionSummary`（`:1625-1643`）分支，显示"打开常用语（当前不可用）"，
   可用性来源接 `common_phrase_panel_snapshot().available`（顺带让 `is_available()` 有消费方）。
5. **失败提示接到 UI**（评审 B3）：报告 sink 现在是空闭包、面板已隐藏、窗口 372px 放不下提示行。
   方案：失败时把面板重新显示并下发原因（文案按失败原因分叉），或按 macOS 做独立提示窗。
6. **设置页挂载点**：条件渲染 `settingsComponent()`；动作列表可见性按 `available`
   （让空壳导出不是死代码，评审 M6）。

### 批次 4（卫生与文档）

7. 门禁开关：`initial_access()` 改精确 `=="1"` + 仅 debug / `runtime-simulation` 构建生效 + 显式日志（评审 M1）。
8. `cancel_pending` 不阻塞按键线程（评审 M3）；"用户自行复制"分支释放快照（评审 M4，补用例）。
9. 静默失败补日志（线程起不来 / 队列中毒 / 归档读失败 / `run_on_main_thread` 失败）；私有 `host.rs` 跑 `cargo fmt`（评审 M10）。
10. 注入脚本护栏（评审 M8）：注入前预清目标、校验 UI 入口与生成产物新鲜度、打印"不要提交"；
    生成器接入 `verify`（重新生成逐字节比对）。
11. 文档：`Testing/WindowsCommonPhrases.md`（含物理键盘同吞用例、真机清单）、`TODO.md`、
    `docs/product-copy.md` 术语登记、`feature/common-phrases/platform-windows.md`（Mac 规范要求）。

## 已拍板决定

1. 插入机制：剪贴板 + Ctrl+V（含快照/恢复/用户新复制不回滚）；
2. 打开方式：只做按键映射动作「打开常用语」；
3. 面板实现：Vue + 浮层窗口（构建期注入私有 UI）；
4. 缺私有组件：隐藏入口（Mac 对齐）；
5. 面板打开期间**物理键盘** Enter/方向键/Back 同吞 —— **接受**，写进测试手册用例；
6. 门禁开关仅开发/走查构建生效（正式包不得绕过会员）；
7. 备份格式本轮**不要求**与 macOS 互通（边界写进文档）。

## 环境与坑（本机实测）

- `pnpm` 不在 PATH：用 `corepack pnpm`（worktree 首次需 `corepack pnpm install --frozen-lockfile`）。
- `src-tauri` 的 cargo 命令需要：`dist/` 存在（否则构建脚本失败）+ `src-tauri/sayall-helper.exe`
  与 `src-tauri/frida-gadget.dll` 存在（gitignore，从主 checkout 的 `src-tauri/` 根复制）。
- 编译 Rust 用 VS Developer PowerShell（`C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat`）。
- **不要用 PowerShell 做文本替换**（曾把 Cargo.toml / capabilities 写坏）；只用 edit/write 工具。
- 删文件走回收站：`Add-Type -AssemblyName Microsoft.VisualBasic` +
  `[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile(...,'SendToRecycleBin')`；
  `Shell.Application.InvokeVerb('delete')` 在本机会挂住，勿用。
- 注入脚本会覆盖公开仓空壳（`crates/sayall-windows-common-phrases/`、`src/private/windows-common-phrases/`）：
  用完必须 `git restore` 还原并清掉注入的私有文件，否则 `git add -A` 会把私有实现提交出去。

## 评审结论索引（两份独立评审，均已记录）

- 私有包：B1（已修）选中永不插入；B2（已修，批次 2）收尾调用点补齐（公开 `reset`/`shutdown` + 私有 `shutdown` 等完在途事务）；B3 失败反馈链断；M1 门禁开关可绕过；M3 `cancel_pending` 阻塞；M4 快照泄漏；M5 刷新事件未接；M6 静默失败无日志；M7 生成器未接 CI；M8 注入护栏缺失；M9 引用文档不存在；M10 fmt 硬伤。
- 公开接缝：B1（已修）`WS_EX_NOACTIVATE` 被 tao 抹掉；M1（待修）映射页 union 缺失；M2（已修，批次 2）认领配对自持（公开侧记账，释放沿配对消费）；M3（已修）收尾/掩码；M4（已修）多屏居中；M5（已修）面板页样式；M6 前端挂载点。
- 已确认无问题：内置内容与 macOS 同哈希、生成产物逐字节一致、七类非法输入全部失败、剪贴板内存所有权路径、工作线程锁无死锁、认领一次性语义、日志不含正文。

## 真机走查清单（出包后）

1. 绑定非语音键到「打开常用语」→ 面板居中显示（**副屏也要验**）；
2. **不抢焦点**：看日志 `COMMON_PHRASES PANEL phase=shown … fg_before/fg_after/self_pid`
   应一致且非本应用；鼠标点面板后再核一次；
3. 逐键插入：OK/左/上/右/下 对应 1–5 条，按下即插入（该键配了双击/长按也要立即响应）；
4. 返回键关闭且不透传；插入后剪贴板恢复原内容；
5. 光标不在输入框 → 失败提示约 5 秒且不插到别处；
6. 面板打开期间遥控器断连/睡眠 → 六个键立即可用（fail-open）；
7. 连续插入、快速连按、面板开关后原映射逐键一致；
8. **退出剪贴板恢复（批次 2 新增）**：插入后立刻从托盘退出应用 → 重开应用检查
   剪贴板应为原内容（不得停在常用语文本上）；日志应见 `COMMON_PHRASES SHUTDOWN
   phase=completed result=services_stopped`。
9. **断连配对（批次 2 新增）**：面板打开期间断遥控器电源 → 面板关闭、按键六个键
   恢复原映射，且无幽灵单击/长按（按一次只响应一次）。
