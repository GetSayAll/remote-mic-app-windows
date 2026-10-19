# 推荐页按钮点击无反应（外链无法跳转）

- 发现日期：2026-10-19
- 状态：已修复（本机调试构建 + 本地测试安装包 verified）
- 影响范围：无线麦 SayAll Windows 版 1.0.0 本地测试包；推荐页内所有
  `target="_blank"` 外链（了解 vokie / 打开官网）。
- 功能点：设置侧栏「推荐」→ 官网页面内按钮。

## 现象

用户在推荐页点击「了解 vokie ↗」「打开官网 ↗」，界面没有任何反应：不跳转、
不打开浏览器、也没有错误提示。

## 复现条件

1. 启动应用，进入侧栏「推荐」；
2. 推荐页正常显示（日志 `navigation result=passed http_status=200`）；
3. 点击任一外链按钮。

稳定复现，与深浅色、窗口尺寸无关。

## 证据

通过 WebView2 远程调试（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`）
在推荐页执行真实点击，捕获到：

```text
EXCEPTION: {"text":"Uncaught (in promise)","exception":"Command plugin:opener|open_url not allowed by ACL"}
Network.requestWillBeSent http://ipc.localhost/plugin%3Aopener%7Copen_url
RESP: 200 http://ipc.localhost/plugin%3Aopener%7Copen_url
ENV: {"hasTauri":true,"hasIpc":true}
```

同时应用诊断日志里**没有任何** `feature=recommendations event=external_link`
记录——说明导航/新窗口处理器从未被调用，点击在页面脚本层就被拦截了。

## 根因

`tauri_plugin_opener::init()` 默认启用 `open_js_links_on_click`，会向每个
WebView 注入一段 IIFE：

```js
window.addEventListener("click", e => {
  // 命中 target="_blank" 的 http/https 链接
  e.preventDefault();
  window.__TAURI_INTERNALS__.invoke("plugin:opener|open_url", { url });
});
```

该脚本对**所有** WebView 生效，包括推荐页子 WebView；但
`src-tauri/capabilities/default.json` 只给 `main` 窗口授权，子 WebView
`recommendations` 不在 `windows` 列表里，于是 IPC 被 ACL 拒绝。脚本已经
`preventDefault()`，所以链接既没走 `on_new_window`，也没有任何可见结果。

## 修复

`src-tauri/src/lib.rs`：

```rust
.plugin(tauri_plugin_opener::Builder::new().open_js_links_on_click(false).build())
```

关闭注入脚本，让推荐页的 `on_new_window` / `on_navigation` 处理器负责：
站外 HTTPS 链接交给系统浏览器（`app.opener().open_url`），子 WebView 内
不跳走。主窗口前端仍然显式调用 `openUrl()`，不受影响。

## 验证

- 修复前（CDP 实测）：点击 → `Command plugin:opener|open_url not allowed by ACL`，
  无 `external_link` 日志。
- 修复后（CDP 实测，调试构建）：点击 → `Page.windowOpen https://vokie.com/?from=sayall.app`，
  无异常；诊断日志 `feature=recommendations event=external_link action=browser result=passed`；
  系统 Chrome 进程在点击同一秒启动。
- 修复后（安装包实测，pid 13332）：同样得到 `external_link action=browser result=passed`，
  点击两次均成功。
- 自动化：`cargo check -p sayall-windows-app --features runtime-simulation`、
  `cargo build`、`cargo test --workspace` 通过；前端测试 378 passed | 11 skipped。

## 未覆盖

- 推荐页「查看详情」类内部导航（当前官网卡片没有该按钮）。
- 断网 / 主文档 5xx 的失败页未在真机触发。
