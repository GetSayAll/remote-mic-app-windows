# 注册应用（AUMID）目标：已运行时重新打开会新开实例/窗口，而不是切回已有窗口（2026-10-02）

## 现象

用户实测（按键映射目标均为 `shell:AppsFolder\...` 注册应用）：

- `Microsoft.Office.WINWORD.EXE.15`（Word）：按下后出现**新的** Word 窗口/文档，而不是切回已打开的文档；
- `Microsoft.Office.POWERPNT.EXE.15`（PowerPoint）、`Kingsoft.Office.WPS`（WPS）同样「出现新的应用」。

而 UI 上「打开应用」的悬停提示承诺的是「已运行则切到该应用窗口，未运行则启动」。

## 根因（代码路径）

`crates/sayall-windows/src/registered_apps.rs:234` `launch_registered_app()` **无条件先走激活契约**：

1. `resolve_registered_identity()` 解析出 AUMID（可能还有 exe 路径）；
2. `IApplicationActivationManager::ActivateApplication(aumid)`（失败才回退 `ShellExecuteExW`）；
3. 之后才在 `observe_registered_foreground()`（`:381`）里尝试把窗口前置：`activate_application_window(aumid)` → `activate_executable_path()` → `activate_process_window(pid)`。

对 Word / PowerPoint / WPS 这类应用，第 2 步本身就会**新开**实例或文档/首页窗口；第 3 步执行时新窗口已经存在，于是被前置的正是那个新窗口——用户看到的就是「又开了一个」。

「把已运行的 AUMID 应用切到前台」的能力其实已经具备，只是**没有在启动前调用**：

- `crates/sayall-windows/src/app_launcher.rs:450` `activate_application_window(aumid)`：先按窗口 AUMID 匹配，再按**进程级 AUMID** 收集 PID 后激活；
- `:531` `activate_executable_path(path)`：按 exe 路径匹配运行中的进程；
- `:636` `activate_process_windows()`：枚举目标进程顶层窗口，优先可见窗口，无可见窗口时回退托盘隐藏窗口。

## 修复方案（实现见下节）

在 `launch_registered_app()` 的激活契约**之前**插入「先激活已有窗口」：

1. `activate_application_window(aumid)` 成功 → 直接返回 `Ok`，日志 `phase=prelaunch_activation result=activated source=window|process_aumid`；
2. 否则若解析到 exe 路径，`activate_executable_path(path)` 成功 → 返回 `Ok`；
3. 两者都失败（应用确实未运行）→ 继续现有激活契约 + 读回（现有逻辑不动）。

需要一并确认的点：

- **多窗口选择语义**：`activate_process_windows()` 取 Z 序最前的可见顶层窗口。对 Office 多文档场景，要确认取到的是用户上次使用的那个窗口（可能需要「最近活动优先」的显式选择），并用真机验证；
- **托盘隐藏应用**（实测 WPS 当前全部顶层窗口 `visible=False`）：修复后应能直接恢复并前置，而不是新开。

## 验收清单（修复后）

- [ ] 应用未运行 → 启动（现状行为不变）；
- [ ] 已运行单窗口 → 切回该窗口，不新开；
- [ ] 已运行多文档窗口 → 切回用户上次使用的那个；
- [ ] 已收进托盘/窗口隐藏 → 恢复并前置；
- [ ] 非 AUMID 目标（普通 exe / .lnk 自定义应用）回归；
- [ ] RC001 / RC003 分别记录。

## 证据

- 用户配置 `%APPDATA%\app.getsayall.remote-mic.windows\button-mappings.json`：
  `tv → shell:AppsFolder\Kingsoft.Office.WPS`、`down → shell:AppsFolder\Microsoft.Office.POWERPNT.EXE.15`、
  `up → shell:AppsFolder\Microsoft.Office.WINWORD.EXE.15`、`menu → shell:AppsFolder\WorkBuddy.WorkBuddy`；
- 现场状态：测试时段内 `WINWORD`（窗口「文档1 - Word」）与 `wps`（仅隐藏窗口）均在运行；
- 代码路径见上（本次只做定位，未改动代码）。

## 修复（2026-10-02 已实现，待用户真机复验）

`registered_apps.rs` 的 `launch_registered_app()` 在激活契约**之前**插入预启动激活，
按三级匹配依次尝试，命中即返回：

1. `activate_application_window(aumid)` —— 窗口 AUMID / 进程 AUMID 匹配（`source=app_identity`）；
2. `activate_executable_path(resolved_path)` —— 进程映像路径精确匹配（`source=executable_path`）；
3. `activate_install_directory_family(resolved_path, aumid)` —— **同安装目录同族进程**兜底
   （`source=install_directory`，针对 WPS 补充）：优先 exe 基名出现在 AUMID 里的候选
   （`Kingsoft.Office.WPS` ↔ `wps.exe`），其余同目录候选降级；判定逻辑为纯函数
   `launcher_family_rank`，有单测覆盖（含「相邻目录不算同族」的负例）。

三级都不命中才认为「确实没在运行」，继续原有激活契约 + 读回；未命中原因写入
`phase=prelaunch_activation result=not_running aumid_hit=… executable_path_available=…`
（不含任何路径，遵守隐私规则）。

### 验证证据（本机实测，2026-10-02）

| 场景 | 结果 |
| --- | --- |
| Word 未运行 → 触发 | 正常启动（≈2.1 s），进程从无到有 |
| Word 已运行 → 再触发 | `prelaunch_activation result=activated source=executable_path`，**104 ms**，pid 前后一致（43220 → 43220），无新窗口 |
| WPS 已运行（解析到 `ksolaunch.exe`，文档进程在同目录子目录）→ **修复前** | `result=not_running` → 新开第 2 个文档窗口（2666 ms） |
| 同上 → **修复后** | `prelaunch_activation result=activated source=install_directory`，**125 ms**，pid 集合与窗口集合完全不变 |

复现工具：`cargo run -p sayall-windows --example registered_app_activate_probe -- <target>`
（配合 `SAYALL_GATT_LOG` 落盘判定链；窗口集合可用任意 UIA/窗口枚举工具对比，判定依据是
`phase=prelaunch_activation` 的 `result`/`source` 与目标进程 PID 是否变化）。

## 未验证 / 边界

- 多文档窗口切回「哪一个」仍取现有 `activate_process_windows` 的 Z 序最先可见窗口；
  Office 多文档场景需用户真机确认取到的是上次使用的那个；
- 本机实测覆盖 Word 与 WPS；PowerPoint 与 Word 走同一条精确路径匹配，但按仓库规则
  仍需单独记录 `passed` / `deferred`；
- RC001 / RC003 实体按键链路未测（本次用探针直接调用产品函数）。

## 与「聚焦输入框」工作的关系

「已运行 → 切回已有窗口」是焦点落地的前提：新开窗口时用户上次的输入框并不在新窗口里。建议本 Bug 与聚焦特性的「打开应用后聚焦」按同一真机轮次验收。
