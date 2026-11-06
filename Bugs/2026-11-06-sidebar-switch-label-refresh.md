# 切换侧栏后按键页文案刷新（内部 id 跳成产品名）

- 发现日期：2026-11-06
- 状态：已修复（本机 CDP 实测通过；安装包复验见「验证」）
- 影响范围：无线麦 SayAll Windows 版 1.8.3（`b89c4d4`），所有配置了「打开应用」的按键格；
  版本与页面组合无关，冷启动、切页都会出现。
- 功能点：侧栏导航 / 按键页动作文案 / 预设应用名注册

## 现象

从侧栏切到「我的」「组合动作」「连接」再回到「按键」时，整页内容重新读一遍；
按键格的「打开应用」文案先显示内部目标（如 `打开 netease_music`、`打开 sayall`），
约 0.5–1.5 秒后才跳成产品名（`打开网易云音乐`、`打开无线麦 SayAll`）。
「组合动作」页首帧计数显示 `0`，随后才变成真实条数。

## 复现条件

1. 启动应用（本机 1.8.3），按键页已有至少一个「打开应用」动作（预设或自定义应用）；
2. 点侧栏「组合动作 / 连接 / 我的」，再点回「按键」；
3. 观察按键格文案，或清掉首帧缓存后整页重载。

## 正常预期

- 切页不重建页面，保留滚动位置、选中项与未保存草稿；
- 文案从第一帧起就是产品名，任何时刻都不出现 `netease_music` 这类内部 id。

## 证据（2026-11-06，WebView2 CDP，安装版 1.8.3 / 本机调试构建）

- 整页重载标记（`window.__sayallSentinel`）在切页后仍存在，且无 `Page.loadEventFired`
  ⇒ 不是 WebView 重载，而是页面组件卸载 / 重建。
- 修复前切回按键页的采样（毫秒为点击后时间）：

  ```text
  0ms    （旧页面已卸载，新页面首帧）
  40ms   单击打开 sayall / 单击打开 netease_music / 单击打开 dimagent / 单击打开 workbuddy
  700ms  同上
  1500ms 单击打开无线麦 SayAll / 单击打开网易云音乐 / 单击打开 DimAgent / 单击打开 WorkBuddy
  ```

- 「组合动作」页首帧 `.count-badge` 为 `0`，约 60–100ms 后为真实值 `8`。
- 阳性对照（清掉首帧缓存后重载）：200ms / 300ms / 500ms 仍显示内部 id，900ms 才变产品名；
  保留缓存后同样重载，200ms 首帧即显示产品名。

## 根因

1. `src/lib/bridge.ts` 的 `registerPresetAppNames()` 每次调用 **clear 整张名称表**。
   按键页、组合动作页、我的页各自带不同列表调用它（按键页挂载早期只有「自定义应用」，
   预设列表要等 `listPresetApps()` 扫描完才回来），后到的部分列表把先到的预设名清掉；
   动作文案只能回退到 target 原文（内部 id / 路径文件名）。
2. `src/App.vue` 用 `<component :is>` 直接渲染页面，没有缓存：每次切页都卸载重建，
   页面的挂载读取重新跑一遍，于是首帧必然是空/默认值，文案与计数都会二次跳变。

## 修复

- `src/lib/bridge.ts`：`registerPresetAppNames` 改为**合并**（空 id / 空名跳过），不再清空；
  名称表写入 `localStorage`（`sayall.preset-app-names.v1`）并在模块加载时同步恢复，
  保证页面首帧就能拿到上次已知的产品名（localStorage 不可用时静默降级，行为同修复前）。
- `src/App.vue`：`<component>` 外包 `<KeepAlive :include="…">`，缓存
  `ButtonsPage` / `MacrosPage` / `ConnectionPage` / `ProfilePage`；
  推荐页是原生子 WebView 覆盖层，离开必须卸载，保持不缓存。
- 新增 `clearPresetAppNames()` 仅用于测试重置模块级缓存。

## 验证

- 回归测试（先红后绿）：
  - `keeps known preset names when a later registration carries only a partial list`
  - `ignores blank entries without dropping names already known`
  - `restores preset names from the local first-frame cache on module load`
  - 阳性对照：临时恢复「先 clear」的旧实现，前两条用例以
    `Expected "打开网易云音乐" / Received "打开 netease_music"` 失败；恢复修复后通过。
- 前端全量：`vitest run` **622 passed | 10 skipped**；`pnpm build`（`vue-tsc --noEmit && vite build`）passed。
- CDP 实测（本机调试构建，真实 WebView + 真实 IPC）：
  - 切回按键页：0 / 30 / 80 / 200 / 600ms 五个采样点全部为产品名，不再出现内部 id；
  - 组合动作页二次进入：`.count-badge` 首帧直接是 `8`；搜索框输入「聚焦」后切到「连接」
    再回来，值与过滤结果保留 ⇒ KeepAlive 生效；
  - 清缓存重载 200–500ms 显示内部 id、保留缓存重载 200ms 即产品名（正/负对照）。
- runtime simulation（真实 Tauri IPC 旅程，`-features runtime-simulation` 构建）：
  `scripts/test-windows-runtime-simulation.ps1` **27 steps passed**（含「五个侧栏页面均在
  Windows WebView 中完成导航和渲染」）。
- 安装包复验（`3ea535a`，SHA-256 `470d3068…`）：`verify-local-test-install.ps1 -ExpectRevision`
  全部 PASS（内嵌修订号 `3ea535a5`）；安装版 CDP 采样与上面一致：按键页 0 / 30 / 80 / 200 /
  600ms 全为产品名，组合动作页二次进入首帧即 `8`，整页重载后 200ms 起即为产品名。

## 未覆盖

- 真实 RC001 / RC003 按键链路未在本次改动中回归（改动只涉及前端页面生命周期与文案缓存，
  不触碰 BLE / HID / 注入路径）；按惯例仍需真机确认切页时按键高亮不中断。
- 推荐页、回眸页、设置页仍按原样每次进入重建（本次未纳入缓存）。
- ARM64 未覆盖（本机无 ARM64 工具链与硬件）。

## 隐私检查

未包含个人路径、设备身份、语音内容或凭据；示例使用产品内预设 id 与公开产品名。
