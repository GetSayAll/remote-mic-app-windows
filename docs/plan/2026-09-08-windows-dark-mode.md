# Windows 深色模式适配方案

状态：**已实现；自动测试与浏览器实测 passed，原生运行验收部分 deferred**

基线：`origin/main` `1335b82b0028690340c7a604af058bf0e40958a0`（v0.2.2，2026-09-08 拉取）

## 1. 目标与产品边界

本方案把用户所说的“Windows 夜间模式”定义为 **Windows 应用颜色模式中的深色主题**，不是改变屏幕色温的 Windows“夜间模式/夜间灯光”。

首版目标：

- SayAll 启动时采用 Windows 当前的浅色/深色应用模式；
- “关于”页面提供“系统 / 浅色 / 深色”三档手动选择器，默认“系统”；
- 用户选择立即生效并持久化，重启和升级后保持；
- 选择“系统”时，应用运行期间修改 Windows 颜色模式，SayAll 无需重启即可同步切换；
- WebView 内容、原生标题栏及系统控件保持同一明暗模式；
- 切换主题只改变显示，不重建 Vue 页面，不重启 BLE、音频、Raw Input、按键映射或更新服务；
- Windows 10 1809 和 Windows 11 均保留现有功能与布局。

首版不做：

- 不改变现有紫色品牌强调色，不借主题适配顺手重做视觉风格；
- 不自行控制 Windows 夜间灯光、壁纸、系统强调色或第三方应用主题；
- `sayall-core` 只保存平台无关的三档偏好枚举，不放入 Windows 主题探测或当前有效主题；Windows BLE/HID/音频平台层完全不感知主题。

主题偏好写入现有 `settings.json`，设置 schema 从 2 升为 3；旧配置缺少该字段时迁移为“系统”。该偏好只属于展示配置，不参与遥控器或语音运行时。

## 2. 当前基线与问题

当前 `src/styles.css` 只有浅色值：

- `:root` 固定文字 `#1f2430`、页面背景 `#f4f5f8`；
- 卡片、侧栏、按钮、状态面板和映射编辑器大量使用白色或近白色背景；
- 全文件共有 108 处颜色字面量、83 个不同值；
- 没有 `color-scheme`、`prefers-color-scheme`、主题属性或主题变更监听；
- `tauri.conf.json` 没有强制窗口主题，因此原生窗口保留 Tauri/Windows 默认跟随行为，但 WebView 内容始终为浅色，可能出现深色标题栏配浅色内容。

直接给 `body` 加一条深色背景不足以完成适配：状态徽章、警告、错误、成功、禁用态、映射选中态、遥控器按下态都需要分别保持语义和对比度。

## 3. 参考实现与取舍

### 3.1 本仓库现有能力

项目当前使用 `@tauri-apps/api` 2.8.x。已安装类型声明提供：

- `getCurrentWindow().theme()`：读取窗口当前 `light | dark | null`；
- `getCurrentWindow().onThemeChanged(...)`：监听运行中的系统主题变化；
- `setTheme(...)`：主动覆盖主题，本次不调用，以免破坏“跟随 Windows”。

CSS 的 `prefers-color-scheme` 作为 WebView 首帧和浏览器预览的基础信号；Tauri 窗口事件作为原生运行时的权威同步信号。

官方资料：

- [Tauri 2 Window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/)
- [Microsoft：Windows 应用颜色指南](https://learn.microsoft.com/windows/apps/design/signature-experiences/color)
- [MDN：prefers-color-scheme](https://developer.mozilla.org/docs/Web/CSS/@media/prefers-color-scheme)

### 3.2 同类产品调研

本地参考库 `richlearntodo-debug/vibe-flow`，HEAD `b47f7cdce8b753fade0c64c97332bebe80f17d2d` 的公开文档记录了浅色、深色和跟随 Windows 三档主题，以及运行中即时切换不重启 Host/Bridge/Capture 的边界。

本项目只借鉴以下产品语义，不复制其未开源 UI 代码：

- 主题切换是纯显示行为，不能影响遥控器、语音和按键服务生命周期；
- 深色主题应使用低饱和中性层级，不能靠高饱和描边弥补层次；
- 浅色和深色都要做实际截图与布局检查。

SayAll 采用相同的三档产品语义，但按现有信息架构把选择器放在“关于”页面，不新增只有一个设置项的独立设置页。

## 4. 设计

### 4.1 状态流

```text
settings.json theme_preference = system | light | dark
  ├─ system：Windows 应用颜色模式
  │    ├─ 首帧：WebView prefers-color-scheme → CSS 立即选择调色板
  │    └─ 运行中：Tauri window theme / onThemeChanged
  ├─ light：固定原生窗口与 WebView 为浅色
  └─ dark：固定原生窗口与 WebView 为深色
          ↓
  <html data-theme="light|dark"> → 只重算 CSS 变量
          ↓
  Vue 页面和后台服务均不重建
```

选择“系统”时，浏览器预览使用 `window.matchMedia("(prefers-color-scheme: dark)")` 完成同样的初始化和实时切换。若 Tauri `theme()` 暂时返回 `null`，保留 CSS 媒体查询结果，不强行回落浅色。选择固定浅色或深色时仍保留系统监听，但系统变化不改变当前有效主题；重新选择“系统”时立即采用最新系统值。

### 4.2 前端模块

新增独立的 `src/lib/theme.ts`，职责限制为：

1. 在 Vue 挂载前读取主题偏好、解析有效主题并设置 `document.documentElement.dataset.theme`；
2. 对固定选择调用 Tauri `setTheme("light" | "dark")`，对“系统”调用 `setTheme(null)`，使原生标题栏与 WebView 一致；
3. 原生环境订阅 `onThemeChanged`，浏览器预览订阅 `matchMedia`；
4. 向“关于”页面提供响应式的偏好、保存中状态与错误信息；
5. 保存失败时回滚选择和值，保持上一次有效主题；
6. 返回清理函数，热更新或应用卸载时解除监听；
7. 对读取、初始化、用户选择、保存、变更、回滚和监听失败写结构化日志，不抛错阻断应用启动。

建议事件字段：

```text
feature=theme event=initialized source=tauri|media_query resolved=light|dark
feature=theme event=changed source=tauri|media_query previous=light|dark resolved=light|dark
feature=theme event=preference_changed preference=system|light|dark result=ok|error
feature=theme event=fallback reason=theme_unavailable|listener_failed resolved=light|dark
```

日志不包含个人路径、设备身份或用户内容。前端使用 `console.info/warn` 记录即时事件，并通过 Tauri IPC 将初始化/切换操作的唯一终态写入 `SAYALL_GATT_LOG`；同一 `operation_id` 关联请求、设置落盘与界面应用结果，区分 `passed`、`failed`、原因及耗时。主题失败必须 fail-soft：维持最后一次有效主题，首次失败则由 CSS 媒体查询决定。

`src/main.ts` 在 `app.mount()` 之前初始化主题，避免 Vue 已显示后才切色。`App.vue` 不保存主题业务状态，防止主题变化触发页面组件重建。

主题偏好的权威来源是 Rust `SettingsStore`。前端另在 WebView `localStorage` 保存一份只读启动镜像，`index.html` 在加载应用脚本前用它决定首帧颜色，避免固定深色重启时先出现浅色白闪；主题控制器随后以 Rust 设置校准镜像。镜像损坏或不可用时按“系统”回退，绝不反向覆盖 Rust 权威设置。浏览器预览只用内存默认值模拟 IPC，不把测试偏好冒充桌面端持久化成功。

### 4.3 “关于”页面选择器

- 在应用信息卡与“软件更新”卡之间增加“外观”卡；
- 卡内使用单选语义的三段选择器，顺序固定为“系统 / 浅色 / 深色”；
- “系统”辅助文案为“跟随 Windows 的应用颜色模式。”；
- 点击后立即尝试保存并应用；保存期间禁用三个选项，防止并发写入；
- 保存失败显示就地错误并回滚到此前选项，不用弹窗；
- 支持键盘 Tab 聚焦、方向键切换和屏幕阅读器读取选中状态；
- 不把主题控制塞入托盘菜单，不新增页面导航项。

### 4.4 CSS 语义令牌

把现有颜色按用途收敛成语义变量，而不是用全局反色滤镜。浅色值尽量保持 v0.2.2 当前视觉，深色只覆盖变量。

| 令牌组 | 用途 | 深色方向 |
|---|---|---|
| `--surface-canvas/sidebar/card/subtle/raised` | 页面、侧栏、卡片、状态区、悬浮层 | 中性灰逐级抬高，禁止纯黑大片背景 |
| `--text-primary/secondary/disabled/inverse` | 标题、正文、弱化、禁用、反色文字 | 正文高对比，次级文字仍可读 |
| `--border-default/strong` | 卡片、分隔线、编辑格 | 深色下提高可见度但不形成亮框 |
| `--accent/default-hover/subtle/text` | 主按钮、选中、导航、映射 | 保留品牌紫，分别校准底色与文字 |
| `--status-success/warning/error/pending-*` | 徽章、提示、错误横幅 | 每种状态都有前景、背景和边框/光环 |
| `--mapping-pressed-*` | 实体遥控器按下反馈 | 保持橙色，与选中紫色可区分 |
| `--shadow-card/control` | 卡片与控件阴影 | 深色降低阴影依赖，以表面层级和边框为主 |

实施时处理全部 83 个不同颜色值；允许少量稳定品牌色或透明度仍为字面量，但每个剩余字面量必须有明确用途，不能遗漏在某个页面形成浅色孤岛。

在根节点声明 `color-scheme: light dark`，使滚动区域、表单控件和系统绘制元素得到正确配色。对 `forced-colors: active` 不覆盖系统颜色；焦点轮廓、选中态和禁用态不能只靠颜色区分。

### 4.5 原生窗口

- 不在 `tauri.conf.json` 固定 `theme`；
- 固定选择调用应用级 `setTheme("light" | "dark")`，“系统”调用 `setTheme(null)` 取消覆盖；
- 用 Tauri 当前窗口主题事件同步 WebView，原生标题栏继续由 Windows/Tauri 绘制；
- 验收时必须确认标题栏、窗口客户区和系统菜单在 Windows 10/11 上一致，不能只看浏览器预览。

若 Windows 10 1809 + 当前 WebView2/Tauri 组合出现标题栏不跟随，先以真机日志和截图确认，再在 Tauri Host 做最小修复；方案阶段不预先引入未证实需要的 Windows 私有注册表或 DWM workaround。

### 4.6 图片和图表

- RC003 实物图为透明/实拍资产，保持原图，不做 CSS 反色；
- 应用 logo 保持原色；
- 使用统计条、图例、连线、按下锚点分别映射语义令牌；
- 图片边缘、透明区域和卡片背景在深色下需真图检查，不能只验证 DOM。

## 5. 预计修改范围

批准开发后，预计只修改或新增：

- `src/lib/theme.ts` 与对应单元测试；
- `src/main.ts`：Vue 挂载前初始化；
- `src/lib/bridge.ts`：主题偏好 IPC；
- `src/pages/AboutPage.vue` 与测试：三档选择器、保存状态和错误回滚；
- `src/styles.css`：语义令牌、深色调色板、系统高对比兼容；
- `crates/sayall-core/src/settings.rs`、`src-tauri/src/settings.rs`、`src-tauri/src/lib.rs`：偏好模型、schema 迁移、读写命令及结构化日志；
- 必要的页面测试断言；
- 运行时仿真/验收文档和本路线图状态。

预计不修改：

- `crates/sayall-windows/**`；
- BLE、ATVV、WASAPI、Raw Input、SendInput 和按键门控；
- 安装器、更新器、托盘菜单和应用权限。

若开发中发现必须越过以上边界，停止并另行说明，不自行扩大范围。

## 6. 验证计划

### 6.1 自动验证

- 主题解析：浅色、深色、`null`/异常回退；
- 三档选择可保存、立即生效，失败时回滚且显示错误；
- schema 2/缺字段配置迁移为“系统”，三档设置均能序列化往返；
- 原生事件和媒体查询事件均能实时更新 `data-theme`；
- 重复事件幂等，监听清理后不再更新；
- 浏览器预览不加载 Tauri 时正常工作；
- 主题读取或监听失败不阻断 Vue 挂载；
- 现有 Vue 测试、类型检查和生产构建全部通过；
- 扫描 CSS，禁止新增未登记的浅色专用字面量。

预期命令：

```powershell
pnpm test
pnpm build
scripts/ci-preflight.ps1
```

### 6.2 Windows 真机验收

每个场景都记录 `passed / failed / deferred`，没有实际执行不得写 `passed`。

| 场景 | Windows 10 1809 | Windows 11 |
|---|---:|---:|
| 浅色启动，标题栏与内容一致 | 必测 | 必测 |
| 深色启动，首帧无明显白闪 | 必测 | 必测 |
| 应用运行中浅→深→浅即时切换 | 必测 | 必测 |
| 三档选择立即生效，重启后保持 | 必测 | 必测 |
| 选择“系统”后再次跟随 Windows 变化 | 必测 | 必测 |
| 按键、连接与语音、权限、关于四页截图 | 必测 | 必测 |
| 100% / 125% / 150% 缩放无裁切和布局变化 | 至少 100%、125% | 100%、125%、150% |
| 错误、警告、成功、禁用、选中、实体按下状态可区分 | 必测 | 必测 |
| Windows 高对比模式基本可用 | 冒烟 | 冒烟 |
| 关窗到托盘再恢复，主题为当前系统值 | 必测 | 必测 |
| 主题切换期间持续连接/语音/按键服务不重启 | 必测 | 必测 |

RC001 与 RC003 不需要各自验证配色，但必须至少各完成一次“保持连接时切换主题”的回归，才能确认主题变化没有碰平台运行时；这不等价于重新宣称两种遥控器的全部硬件能力通过。

### 6.3 视觉与无障碍门槛

- 普通文本与背景对比度至少 4.5:1；大号文本至少 3:1；
- 关键控件、焦点、状态边界至少 3:1；
- 错误/警告/成功不能只靠红/橙/绿颜色表达，保留文字或图形语义；
- 深色下不出现纯白大面、不可读灰字、消失的分隔线或荧光饱和描边；
- 浅色模式与 v0.2.2 做截图回归，除令牌整理导致的必要微调外不改变既有视觉。

## 7. 完成条件与批准门

用户已于 2026-09-08 批准按本方案开发，并明确要求在“关于”页面提供“系统 / 浅色 / 深色”三档选择器。

批准开发后的交付条件：

1. 修改严格落在第 5 节边界内；
2. 自动验证通过；
3. Windows 10/11 与 RC001/RC003 相关真机项按事实标记；
4. 日志能区分主题来源、初始化、实时变化和回退原因；
5. 独立功能提交，只含夜间模式适配与对应文档证据。

## 8. 实现与验证记录（2026-09-08）

已实现：

- “关于”页面三档选择器、即时切换、保存中禁用与失败回滚；
- `AppSettings` schema 3 与旧设置默认迁移为“系统”；
- Tauri 主题偏好读写命令及 `SAYALL_GATT_LOG` 结构化记录，以同一 `operation_id` 关联请求、落盘和唯一界面终态；
- Tauri 应用级固定主题/取消覆盖、系统主题事件和浏览器媒体查询；
- 首帧 `localStorage` 镜像与 Rust 权威设置校准；
- 浅色/深色语义令牌、四页状态色、焦点与高对比模式基础样式。

验证结果：

| 项目 | 结果 | 证据 |
|---|---|---|
| `scripts/ci-preflight.ps1 -Full` 7 步 | passed | 前端测试、构建、Rust fmt/workspace test/check、Windows runtime-simulation release 构建全部通过 |
| Vue/Vitest | passed | 9 个文件、52 项测试；含三档选择、系统变化、固定深色、保存失败回滚 |
| Rust workspace | passed | 121 项通过、2 项既有条件测试 ignored；含 schema 迁移与三档序列化/持久化 |
| 正式前端构建 | passed | `vue-tsc --noEmit` 与 Vite production build |
| Windows Chrome 1029×732 渲染 | passed | 四页深色无横向溢出，浅/深/系统唯一选中，系统浅→深实时跟随，console error/warn 为 0 |
| 基础对比度探针 | passed | 浅色主文字/页面 14.23:1、次级文字/卡片 4.96:1、选中文字/强调色 4.93:1 |
| Windows Tauri/WebView 仿真运行 | deferred | 本机已有安装版 SayAll 运行，单实例守卫拒绝测试程序；按部署规则未强杀正在连接的应用 |
| Windows 10/11 原生标题栏、重启保持 | deferred | 需可正常退出安装版后的独占运行窗口 |
| RC001/RC003 保持连接时切换主题 | deferred | 本轮未打断用户当前遥控器会话做型号级验收 |

浏览器 QA 使用系统 Chrome，因为本会话未提供 Browser 插件且 Playwright 自带 Chromium 未安装；通过 Playwright 1.62.1 驱动现有 Chrome，未修改项目依赖。
