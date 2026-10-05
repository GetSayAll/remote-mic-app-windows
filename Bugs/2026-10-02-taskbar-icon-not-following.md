# 任务栏图标不跟随应用图标切换

- 发现日期：2026-10-02
- 状态：等待真机验证（安装包内应用内确认 deferred）
- 影响范围：无线麦 SayAll Windows 版 0.5.0 本地验证包 v2（SHA-256 `463c4835c0d8900e09786f0785fd7d686bfd6562d33a2740a22a4786990367ba`）；Windows 11（本机 2560×1440、100% 缩放）；与 RC001/RC003 无关；与第三方工具无关
- 功能点：设置页「应用图标」（标准 / 几何鸭）切换 → 主窗口图标
- 现象：在设置里切换应用图标后，标题栏图标与通知区域托盘图标都跟着变了，**任务栏按钮图标不变**（仍显示默认图标），Alt-Tab 同样不变
- 复现条件：
  1. 安装并启动本地验证包 v2；
  2. 打开设置页，把「应用图标」从「标准」切到「几何鸭」（或反向）；
  3. 观察任务栏按钮：图标不跟随；标题栏与托盘已跟随。
- 正常预期：任务栏按钮图标（以及 Alt-Tab）与标题栏、托盘使用同一张所选图标，切换后立即更新
- 证据：
  - 源码核对（tao 0.35.3，`platform_impl/windows/window.rs:882`）：`set_window_icon` 只写 `IconType::Small`（= `ICON_SMALL`，标题栏用），
    `set_taskbar_icon` 才写 `IconType::Big`（= `ICON_BIG`，任务栏 / Alt-Tab 用）；
    `tauri-runtime-wry 2.11.4`（`src/lib.rs:3565`）的 `WindowMessage::SetIcon` 只调前者，Tauri 的公开路径到不了 `ICON_BIG`。
    另外 tao 注册窗口类时 `hIcon`/`hIconSm` 均为空（`window.rs:1365`），tauri/tao 也未设置进程 AppUserModelID（rg 全仓 0 命中）——
    因此任务栏按钮在修复前既没有窗口级 `ICON_BIG`，也没有可跟随的类图标。
  - 单元测试 `src-tauri/src/app_icon.rs::tests::taskbar_icon_follows_only_when_icon_big_is_written`：
    只写 `ICON_SMALL` 时读回 `ICON_BIG == 0`（根因可复现）；写 `ICON_BIG` 后读回非零且再次切换换新句柄。
  - 真机探针 `src-tauri/examples/taskbar_icon_probe.rs` + `Testing/probe-taskbar-icon.ps1`（2026-10-02 本机 100% DPI，抓 `Shell_TrayWnd` 按颜色统计像素）：
    | 抓图时刻 | 探针写入 | Cyan | Magenta |
    | --- | --- | --- | --- |
    | tc=2s | 无图标（exe 默认图标） | 591 | 0 |
    | tc=12s | 只写 `ICON_SMALL`（洋红 16px） | 586 | 1080 |
    | tc=20s | `ICON_BIG`+`ICON_SMALL`（青色 32/16px） | 1666 | 0 |
    | tc=28s | 再切回`ICON_BIG`+`ICON_SMALL`（洋红） | 586 | 1080 |
    每档跳变 ≈1080 px（一张 32×32 图标 ≈1024 px + 抗锯齿），说明任务栏按钮确实按窗口图标重绘，且连续切换都跟随。
  - 探针 `WM_GETICON` 读回：`small_only=(0, 5441013)`、`big_and_small=(12125689, 1376387)`、`switched=(3081719, 5441013)`。
- 根因（已确认）：Windows 任务栏按钮取窗口的 `ICON_BIG`；Tauri/tao 的 `set_icon` 只写 `ICON_SMALL`，运行期从未写过 `ICON_BIG`。
  标题栏与托盘分别读 `ICON_SMALL` 与托盘句柄，所以只有任务栏（和 Alt-Tab）不动。
  仍未知边界：安装版是否因任务栏按钮的图标缓存出现延迟重绘（本次探针未覆盖安装版进程）。
- 修复：`src-tauri/src/app_icon.rs` 新增 `#[cfg(windows)] mod window_icons`，在 `apply` 里除跨平台 `window.set_icon` 之外，
  用 `CreateIconIndirect` + `WM_SETICON(ICON_BIG/ICON_SMALL)` 自己写两份图标（`ICON_BIG` = 256px 窗口图，`ICON_SMALL` = 按 DPI 的托盘档），
  句柄用进程级 `Mutex<Option<(OwnedIconHandle, OwnedIconHandle)>>` 保活、换新后才释放上一代；`src-tauri/Cargo.toml` 增加 `Win32_Graphics_Gdi` feature。
- 验证：
  - `cargo fmt --all -- --check` → `passed`
  - `cargo test --workspace` → `passed`（含 `app_icon::tests::taskbar_icon_follows_only_when_icon_big_is_written`）
  - 真机探针（真实 Windows 桌面 + 真实任务栏 `Shell_TrayWnd`）→ `passed`（上表四档颜色跳变，连续两次切换都跟随）
  - 安装包内应用内确认（用户在设置页切换后目视任务栏）→ `deferred`（本机装着 v2 正式版且正在运行，单实例互斥无法并跑开发版；本次不出包，等下一次包内确认）
- 隐私检查：记录内无设备身份、无语音内容、无个人路径（仅仓库相对路径与 `%TEMP%` 探针输出目录）/凭据

## 2026-10-03 追加：开始菜单 / 桌面 / 固定到任务栏的快捷方式图标不跟随（已修）

- 现场（同一操作人，安装包 0.5.0 修订 `6693a53`）：切换应用图标后，任务栏按钮与托盘已经跟随，
  但**开始菜单磁贴与桌面快捷方式图标**仍是旧图标（对比图见会话记录）；操作人据此判断「任务栏/
  开始菜单没改成功」。
- 复测结论（UI 自动化直接点设置页 + 抓像素判定）：
  - 运行中的任务栏按钮**本就跟随**：「几何鸭 ⇄ 默认」双向切换后 0.9 s 内按钮图形改变
    （深色圆角方块 dark=1076 ↔ 绿色条带 green=40；3 s 后稳定），窗口 `ICON_BIG`/`ICON_SMALL`
    句柄绘制出的指纹与所选资产（`faceted-duck-256.png` / `icons/32x32.png`）一致。
  - 真正缺口是快捷方式：`IconLocation=,0` 取的是 exe 内嵌图标，运行期不可能跟随切换；本文件
    最初记录把它当作「安装产物不可改」，操作人 2026-10-03 明确要求改成跟随。
- 修复：新增 `src-tauri/src/shortcut_icons.rs`——
  - 把所选风格的 `.ico` 落到 `%LOCALAPPDATA%\SayAll\icons\`（`standard` = `icons/icon.ico`；
    `faceted-duck` = `icons/app-icons/faceted-duck.ico`，由
    `scripts/generate-app-icons.py --ico-only` 从已提交 PNG 派生，多尺寸 16/20/24/32/48/256）；
  - 只改写**目标 exe 等于当前进程 exe** 的快捷方式（开始菜单 / 用户与公共桌面 / 「固定到任务栏」
    目录下的全部 `.lnk` 里匹配者），随后 `SHChangeNotify(SHCNE_UPDATEITEM)` + `SHCNE_ASSOCCHANGED`
    刷新 shell 图标缓存；
  - 日志 `app_icon action=sync_shortcuts phase=completed ... considered/updated/failed`（只记数量，
    不记路径）；`app_icon::apply` 的启动对账与设置页切换两条路径都会走到。
- 验证：
  - `cargo test --release -p sayall-windows-app shortcut_icons` → **4 passed**，含真实 COM 用例
    （`IShellLinkW` 造 `.lnk` → 改写 → 读回 `IconLocation`）与阴性对照（目标不是本程序的 `.lnk`
    必须原样不动）。
  - `cargo test --release -p sayall-windows-app` → **55 passed / 0 failed**。
  - 真机（本机安装包内，2026-10-03 00:34）：应用启动与操作人在设置页的两次实时切换都记录
    `sync_shortcuts ... considered=13 updated=2 failed=0`；两个快捷方式的 `IconLocation` 实测已指向
    `%LOCALAPPDATA%\SayAll\icons\sayall-faceted-duck.ico,0`。
  - **开始菜单磁贴 / 桌面图标刷新后的目视确认 deferred**：需要操作人目视（shell 缓存已发通知，
    必要时关掉开始菜单再打开；若仍不刷新，再补刷新手段）。
- 边界：安装包与 exe 内嵌图标仍是安装产物，不随切换变化；`%LOCALAPPDATA%\SayAll\icons\` 属于应用
  数据，卸载时随其余用户数据保留。
- 隐私检查：本轮记录无设备身份、无语音内容、无凭据；只出现 `%LOCALAPPDATA%` 等环境变量表达与
  探针临时目录。

