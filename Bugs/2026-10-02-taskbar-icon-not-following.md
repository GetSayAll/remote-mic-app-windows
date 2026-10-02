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
