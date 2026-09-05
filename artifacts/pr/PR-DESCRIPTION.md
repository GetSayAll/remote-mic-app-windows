# PR：按键映射功能完成（Mac 式三列手势 + 门控吞键 + 画布高亮）

分支：`codex/key-mapping-mac-parity`（两个提交：`2dd5f98` 功能 + `bc7b7b3` rustfmt 对齐）

> **推送说明**：本分支基于本地 `main`，而本地 `main` 领先 `origin/main` 4 个未推送提交
> （`1803421` 移除统计页 / `cef24d3` 和弦间隔 20ms / `14e9605` BLE 自愈与粘键修复 /
> `c204b79` 文档）。两种推法：
> ① 先推 `main`（4 个提交），再推本分支 → PR 只含本分支 2 个提交（推荐）；
> ② 直接推本分支 → PR 会连带那 4 个提交（内容无冲突，只是范围更大）。
> 补丁文件在 `artifacts/pr/0001-*.patch`、`0002-*.patch`（git am 即可，或直接推分支）。

## 做了什么

### 1. 按键映射功能做完（此前只保存不执行）

- **数据模型 v2**：每键三列（单击/双击/长按）+ 总开关；旧版单动作 `button-mappings.json`
  自动迁移为单击列（真机验证：用户既有 OK→Enter 配置迁移后正确显示与编辑）。
- **手势引擎** `button_gestures.rs`：完整移植 Mac `RemoteButtonGestureRecognizer` 语义——
  双击窗口 300ms、长按 550ms、连发起始 350ms（返回 50ms / 方向与音量 100ms）；
  **按配置动态启用**：未配置双击/长按时单击在按下沿零延迟触发；语音键不参与映射
  （保持按下开始/释放结束）。
- **门控吞键** `key_gate.rs`（WH_KEYBOARD_LL）：已配置映射的按键其原始键入被吞掉
  （替换语义，Mac `KeyboardEventSuppressor` 同款预测式武装模型）——VK 0xFF 厂商键
  （返回/电源/音量）直接归因；其余键由 Raw Input 监听器的 HID 报文武装 + 60ms 有界
  等待归因；DOWN 漏进 OS 则 UP 必放行（防粘键规则）；注入事件免疫；钩子链头 bump；
  未配置映射/总开关关闭一律透传（开箱行为与现状完全一致）。
- **映射引擎** `button_mapping.rs`：双源合并（监听器 HID/透传键盘边沿 + 门控被吞边沿），
  动作为 SendInput tap，监听器停止/设备移除统一释放；门控未运行时自动退化为观察模式
  （不注入，防双输入）。
- **架构实证（新调查）**：双线程探针两轮证实 **LL 钩子吞掉的键盘事件不会再投递
  Raw Input**——引擎因此采用双源合并架构。见
  `docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md` 与仓库根规则
  "延迟/时序类改动必须持锁实测"。
- **监听自愈**：随应用自动启动 + 每 10 秒重试（用户显式停止不重启）；设备热移除
  （RIDEV_DEVNOTIFY）触发释放。真机验证：RC003 已配对主机开机即"按键监听已就绪"。

### 2. 按键高亮对标 Mac（直观性）

按键页重构为 Mac `RemoteMappingCanvas` 对标画布（锚点/目标坐标逐键移植）：
遥控器实物图居中 + 12 张按键卡 + 语音卡 + 贝塞尔连线；**三态高亮**——实体按下中
=橙色（照片锚点同步亮橙点）、UI 选中=强调色、普通=中性；语音键卡片随语音会话点亮；
单击/双击/长按单元格点击即编辑（面板滚动入视口，预设芯片 + 自定义快捷键键盘录入）；
页脚含吞键/泄漏/触发计数、"锁定当前按键"（默认开，按遥控器不抢编辑焦点）、恢复默认、
手动启停监听。

### 3. 全页面布局紧凑化（参考 Mac）

默认窗口（1120×800）初始非滚动状态下四页全部元素在视口内（UIA 遍历实测：
连接 73/0、按键 105/0、权限 49/0、关于 43/0 个折叠线下元素）；连接页重组为
"蓝牙+按住说话快捷键 / 语音输出"双栏，微信输入法步骤收进折叠区；默认页改为按键
（对齐 Mac 页序）。字号全页 ≥12px。

## 对比截图（自测产物，已入仓库）

见 `docs/screenshots/2026-09-05-buttons-mapping/`（含 `README.md` 逐项对比结论）：

| Windows（本 PR） | Mac 参考 |
| --- | --- |
| `windows-buttons-initial.png` | `mac-key-mapping-reference.png` |
| `windows-buttons-editor.png` | 同上（编辑面板） |
| `windows-connection.png` | `mac-connection-reference.png` |
| `windows-permissions.png` / `windows-about.png` | — |

结构逐项对齐：画布布局、三态高亮、三列触发单元格、编辑面板、页脚开关；有意差异
（打开 APP 动作、连发开关、语音键模式分段控件、电量）已在 README 标注。

## 验证状态（按仓库词汇表）

**passed（本机 Windows 实测）**
- `cargo test --workspace`：93 项全部通过（含手势状态机、门控决策、引擎双源合并、
  旧配置迁移、IPC 契约）。
- `pnpm test`（vitest）：18 项通过；`pnpm build`（vue-tsc + vite）通过。
- `cargo fmt --all -- --check` 干净。
- **CI 运行时仿真旅程（`scripts/test-windows-runtime-simulation.ps1`）11 步全过**：
  含按键画布 36 单元格渲染、映射保存热加载经真实 Tauri IPC、
  "按键监听已就绪"随应用自愈启动。（注：本机 VM 上应用退出码偶发为空——
  父提交同样复现，属 WebView2 teardown 环境问题，非本 PR 回归。）
- 真机：RC003（已配对）开机自愈启动 Raw Input 监听就绪；四页 UI 初始视口全覆盖；
  画布/编辑面板/页脚对真实迁移配置渲染正确（UIA + OCR + 截图核对）。

**deferred（需实体按键或另一台设备）**
- 实体遥控器按键回路（按下高亮、吞键、单击/双击/长按注入手感）：RC003 就绪，
  需人手按遥控器复核；RC001 未在本机配对。
- 已知边界：映射键与物理键盘同键在 60ms 归因窗口内可能被误吞（VVC 同款取舍）；
  管理员权限前台窗口中 UIPI 使钩子不可见，映射自动退化为观察模式。

## 文档同步

- `ATTRIBUTION.md`：新增 Mac 手势/画布/抑制器语义移植来源、LL 吞键实证条目。
- `docs/architecture/windows-tauri-roadmap.md`：阶段 C 状态更新为映射功能完成。
- `contracts/ipc/windows-runtime.json`：`rawInput.activeButtons` + `buttonMapping` 快照。
