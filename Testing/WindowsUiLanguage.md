# Windows 应用语言（中英）验收

本手册覆盖设置页「通用 → 应用语言」及全界面中英切换，参考 Mac 最新实现。自动化只能
证明前端渲染与词条完整性；**Windows 真机、托盘菜单和窗口标题必须在安装包内逐项实测**。

## 边界

- 有效语言只有 `zh-Hans` / `en`；`system` 按 Windows 显示语言解析（中文主语言 → `zh-Hans`，
  其余 → `en`）。切换立即生效，不重挂载页面、不丢失当前页/向导步骤/编辑状态。
- 用户数据不翻译：遥控器/音频端点名称、应用名、组合动作名称、账号名、外部错误原文。
- 英文术语以 `docs/product-copy.md` §2 英文术语表为准；导航/页面标题走 `nav.*`，
  内容同名文案（Recommended、Set Up、Connect）不得被导航词条覆盖。
- 原生外壳：主窗口标题、托盘 tooltip、托盘菜单「显示主界面 / 退出」随语言切换；
  每次切换必须重建新菜单项，不能复用已挂载项（Mac 2026-08-01 重复挂载异常）。

## 自动化检查

```powershell
corepack pnpm test
corepack pnpm exec vue-tsc --noEmit
corepack pnpm build
cargo fmt --all -- --check
cargo test -p sayall-windows-app --lib system_ui_language_maps_to_supported_app_languages
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci-preflight.ps1
```

`src/lib/i18n.test.ts` 检查：所有 `t("…")` / `tNav("…")` 字面量有英文词条；各 fragment
无同 key 不同译文；中文模式返回原文、英文模式返回英文。

## 真机用例

前置：使用本地测试安装包；先记录 `%APPDATA%\app.getsayall.remote-mic.windows\settings.json`
中的 `application_language`（不存在 = `system`），测试结束后恢复用户原值。

| # | 操作 | 预期 |
| --- | --- | --- |
| 1 | 中文模式启动 | 侧栏、六个页面、设置页、向导、弹窗、错误提示全中文；「通用」各行**没有副标题** |
| 2 | 设置 → 应用语言 → English | 立即切英文，不重挂载：当前页/滚动位置/编辑状态保留；窗口标题变 `SayAll`；托盘 tooltip 变 `SayAll`；托盘菜单为 `Show Main Window` / `Quit` |
| 3 | 英文模式逐页检查 按键 / 组合动作 / 连接 / 推荐 / 权限 / 设置 | 侧栏为 Buttons / Routines / Connection / Picks / Permissions / Settings；页面标题为 Button Mapping / Routines / Connection & Voice / Picks / Permissions & Privacy / Settings；页面文本、按钮、tooltip、aria-label、错误文案无中文残留；`简体中文` 语言选项除外；推荐页加载 `/en/recommendations/` |
| 4 | 英文模式检查按键页全按键支持确认弹窗、连接页输入工具、权限页诊断、设置页会员/更新/问题反馈 | 弹窗与固定文案全英文；用户数据/外部错误原文不算残留 |
| 5 | 英文模式向导 ①–⑦（重新运行向导） | 阶段条 Prepare › Set Up › Try It；步骤标题、检查卡、门禁提示、失败文案全英文；切回中文立即恢复 |
| 6 | 中文 ↔ English 连续切换 10 次 | 无异常、无重复/叠加托盘菜单项；每次菜单仍只有两项；切回中文后标题/托盘/页面全部恢复 |
| 7 | 关闭应用后重启（English 持久化） | 首帧即英文；窗口标题与托盘菜单英文；设置页选中 English |
| 8 | 选择「跟随系统」，在 Windows 中文与英文显示语言下各重启一次 | 分别得到 `zh-Hans` / `en`；托盘与页面一致 |
| 9 | 英文模式打开托盘菜单 → Show Main Window / Quit | 菜单事件仍有效；Quit 正常退出，不残留托盘图标 |
| 10 | 点击「检查更新」失败或语言保存失败等错误路径 | 界面显示英文固定文案；原始技术错误只进日志/控制台，不直出中文原文 |

## 结果记录

- 自动化：`corepack pnpm test` — 31 files / 505 passed, 10 skipped（2026-10-24）；
  `vue-tsc --noEmit`、`vite build`、`cargo fmt --check`、语言映射单测通过。
- `ci-preflight`：7/7 通过（含 `cargo test --workspace`、`cargo check --workspace`、
  `cargo check -p sayall-windows-app --features runtime-simulation`）。
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`，
  13,051,463 字节，SHA-256 `5DDC84CDB2D5EF7C6805C9B216AA2243366CF5F4849FC6F6A2FE52AA496DE982`
  （构建于合并 `origin/main` 的提交 `353043b`，安装版日志读回同一源修订号）。
- 前端英文渲染：浏览器预览逐页扫描 按键 / 组合动作 / 连接 / 推荐 / 权限 / 设置 的
  可见文本与 `title` / `aria-label` / `placeholder` / `alt`，除「简体中文」选项与
  用户数据（演示组合动作名「打开记事本并聚焦」、应用名「记事本」）外无汉字残留。
- 英文词条按 Mac 最新代码再校对（2026-10-24）：侧栏 Picks；页面标题 Connection & Voice / Permissions & Privacy；公共词条统一 Voice Button、Voice Typing Check、Single Press / Double Press、Choose、Not Set、Back / Continue、Restore Defaults、Voice devices；组合动作仍按组合动作包用 Routines。
- 安装包真机（Windows，系统显示语言中文）：
  - 中文启动：窗口标题「无线麦 SayAll」；日志
    `native_language ... language=zh-Hans title=passed tray_menu=passed`；托盘菜单
    「显示主界面 / 退出」（截图 `tray-menu-zh.png`，224×76，两项）。
  - 设置切 English：即时英文（UIA 读到 `Settings` 页与 `Buttons … Settings` 导航），
    窗口标题变 `SayAll`；日志 language=en、title/tray_menu=passed；托盘菜单
    `Show Main Window / Quit`（截图 `tray-menu-en.png`，305×76，两项）。
  - 连续 12 次中英切换：每次标题中/英切换，14 条 `event=native_language` 日志全部
    `terminal_result=passed`；再次打开托盘菜单仍为 305×76、仅 `Show Main Window` 与
    `Quit` 两项（截图 `tray-menu-en-after-12.png`），无重复挂载。
  - 托盘 `Show Main Window` 与 `Quit` 事件在重建菜单后仍有效；Quit 后进程数为 0。
  - English 持久化重启：首帧标题 `SayAll`，日志加载 `preference=en`、原生 language=en。
  - 选回 System Default：立即回到中文标题与中文导航；重启后仍 `preference=system`、
    language=zh-Hans。
- **deferred**：真机向导 ①–⑦逐页英文视检（为不破坏用户已完成的向导状态，只跑了
  140 个向导组件测试与英文无汉字临时验证）；把 Windows 显示语言改为英文后的
  `system` 模式（会改动系统语言并需重启，仅用 Rust 语言映射单测覆盖 `0x0409 → en`）。
- 已知**不在本功能范围**：用户数据/第三方应用名不翻译；Windows 系统级错误原文可能随系统语言。

