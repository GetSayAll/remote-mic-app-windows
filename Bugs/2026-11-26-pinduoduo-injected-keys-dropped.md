# 拼多多商家版里映射的方向键/回车/字母无效（目标以管理员运行，UIPI 丢弃注入）

- 发现日期：2026-11-26；根因确认 2026-11-27
- 状态：已确认根因（UIPI）；诊断终态与界面提示已补；2026-11-27 决定不提供提权注入路径（不支持）
- 影响范围：用户现场 1.9.7（`source_revision=98fd2a2…`）与 1.9.9（`6d24ee3…`）、
  Windows 11 build 22631、RC003（报告层接管 + 注入）、第三方 App「拼多多商家版」
  （窗口类名哈希 `4aafa526`，以管理员身份运行）；RC001 未涉及
- 功能点：按键映射 → `SendInput` 注入（`button_mapping` → `SendInputRuntime::tap`）
- 现象：只有拼多多商家版里方向键/回车/字母无效；音量±、打开微信、语音可用；其他 App 正常
- 复现条件：目标 App 以管理员身份运行（High IL）时，一律复现
- 正常预期：映射键注入到当前前台 App，并在该 App 内产生对应动作

## 根因（2026-11-27 确认）

**拼多多商家版以管理员身份运行（high 完整性），Windows UIPI 把普通权限无线麦注入的
模拟输入丢弃了。** 不是注入形态、不是目标 App 的钩子过滤、不是钩子宿主掉线。

1.9.9 现场日志（用户复验）按前台窗口分组，`map_target` 是新增的只读探针：

| 前台窗口 | 注入次数 | `fg_integrity` / `elevated` | `send_input` | `echo_delta` |
| --- | --- | --- | --- | --- |
| 拼多多（`4aafa526`，pid 27464） | 26 | **high / true** | `form=scancode`，`sent=2/2`（或 4/4），`result=ok`，`win32_error=0` | **26 次全 0** |
| 微信（`493dd032`） | 33 | medium / false | 全部 `ok` | 30×2、3×4 |
| 无线麦自己（`1656eb49`） | 6 | medium / false | 全部 `ok` | 6×2 |
| 网易云（`58abc5f7`） | 1 | medium / false | `form=virtual`，`ok` | 2 |

补充事实：

- `input_env own_integrity=medium own_elevated=false`：无线麦本身普通权限；
  `low_level_hooks_timeout_ms=25000`（不是钩子超时被摘）。
- 拼多多窗口期间 `hook_calls_total` 67 → 249 持续增长：钩子在工作，物理键仍被看到。
- `fg_focus_pid == fg_pid`：焦点没有被托管到别的进程。
- 助手报告层 `clears_ok=58 / clears_fail=0`：没有原始 HID 报告泄漏，
  所以"音量键可用"不能由泄漏解释（UIPI 会把音量注入一起丢掉；该次测试并不在拼多多窗口内）。
- **实测反例：UIPI 拦截时 `SendInput` 仍返回满数且 `GetLastError=0`。**
  这条推翻了"`SendInput` 被 UIPI 拦会返回 0"的文档印象，也说明
  `result=ok` 不能当成功判据——只能看注入回声 + 目标完整性。

早期 1.9.7 日志（2026-11-25/26）已给出同一模式：拼多多窗口 120 次注入 114 次
`echo_delta=0`，其他窗口 150 次里 149 次 `echo_delta=2`；本次新增字段补上了
"目标 high IL" 这一决定性事实。

## 修复（诊断终态 + 用户提示）

- `button_mapping`：回声为 0 且目标完整性高于本进程时，落
  `map_inject result=dropped echo_delta=0 reason=uipi_target_elevated`，并累计
  `uipi_blocked`；`ButtonMappingSnapshot` 与诊断报告（schema v5）带上该计数。
- 按键页：`uipiBlocked > 0` 时给一句用户语言——"当前窗口的应用以管理员身份运行时，
  Windows 会拦截无线麦发送的按键。请取消它的「以管理员身份运行」后再试。"
- `LOGGING.md` 补判读口径与"result=ok 不代表送达"的实测边界。
- 不改注入行为；不把 `SendInput` 换成鼠标/滚动（同样被 UIPI 拦）。

## 产品边界（2026-11-27 用户决定）

- 目标进程以高完整性（管理员）运行时，`SendInput`、`PostMessage`、UIA 都会被 UIPI 拦，
  改鼠标/滚动动作同样无效。
- **不通过已提权的助手代发按键，也不新增其它提权注入路径。** 这是明确的“不支持”边界，
  不是待办：遥控器按键只投递给普通权限的目标窗口。
- 用户侧做法是让目标应用以普通权限启动；取消「以管理员身份运行」后必须完全退出
  （含托盘/更新器进程）再启动才生效。若该应用的清单强制管理员，则无线麦无法向它发送按键。

## 验证

- `cargo fmt --all -- --check` passed。
- `cargo test --workspace` passed（含新增的 `uipi_only_blocks_targets_strictly_above_own_integrity`、
  `uipi_injection_check_reads_the_target_probe`、`shortcut_injection_note` 的 UIPI 分支）。
- `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- 前端：`vitest run` + `vue-tsc --noEmit` + `vite build`（含按键页提示的“出现/不出现”两例）。
- **现场解法 deferred**：2026-11-27 用户反馈“已去掉管理员权限”，但复验日志
  （06:48–06:57 片段）仍是 43/43 `fg_integrity=high fg_elevated=true`、`echo_delta=0`，
  说明实际运行的进程还是提权态（未完全退出 / 其它启动入口 / 清单强制）。
  按「产品边界」一节确认启动方式；若清单强制管理员，则不支持。
  解除提权后日志应为 `map_target fg_integrity=medium`、`map_inject result=queued echo_delta>0`。

## 隐私检查

本文不含个人路径、蓝牙地址、设备身份、窗口标题明文（只写类名哈希）、输入文字或凭据。
