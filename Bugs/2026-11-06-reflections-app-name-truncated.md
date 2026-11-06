# 回眸应用栏与记录行把长应用名截成省略号

- 发现日期：2026-11-06
- 状态：已修复（自动化与浏览器布局实测 passed；安装包复验 deferred）
- 影响范围：无线麦 SayAll Windows 1.8.3 x64；回眸页；与遥控器型号无关
- 功能点：回眸 · 应用栏 chip · 记录行应用名
- 现象：应用名「腾讯电脑管家截图工具」在应用栏显示为「腾讯电脑管家...」，在记录行同样被省略号截断
- 复现条件：回眸页存在长产品名（本次为 `QMCapture64.exe` 映射后的「腾讯电脑管家截图工具」）；
  应用栏 chip 固定 148px、记录行应用列固定 126px，名称超宽即截断
- 正常预期：应用名完整显示；宽度不足时换行，不出现省略号或裁字
- 证据：
  - 用户截图（2026-11-06）：应用栏与记录行均显示「腾讯电脑管家...」
  - 修复前 `ReflectionsPage.vue`：`.app-chip-label strong` / `.row-app-name` 同时使用
    `overflow: hidden`、`text-overflow: ellipsis`、`white-space: nowrap`
  - 浏览器布局实测（真实 SFC 样式，839px = 1066×800 最小窗口的内容区宽度）：
    修复前名称可用宽度 89px（chip）/ 94px（记录行），「腾讯电脑管家截图工具」13px 实测 130px、
    12.5px 实测 125px，必然截断
- 根因：名称长度超过固定宽度，且样式明确选择省略号截断；没有为长产品名预留宽度或换行路径
- 修复：
  - `.app-chip` 改为 `min-height: 48px` + `height: auto`，允许名称换行时撑高
  - 应用 chip 改为内容自适应：`width: auto`、`min-width: 120px`、`max-width: 190px`，
    `flex: 0 1 auto` 允许窗口变窄时收缩并换行；「全部应用」保持 `min-width: 156px`
  - `.app-chip-label strong` 与 `.row-app-name` 去掉 `overflow: hidden` / `text-overflow: ellipsis` /
    `white-space: nowrap`，改为 `white-space: normal` + `overflow-wrap: anywhere`
  - 记录行应用列由 126px 调整为 164px，使该长产品名在最小窗口下仍能单行完整显示
- 验证：
  - 新增 `src/pages/reflections-app-name-layout.test.ts`：修复前 3/3 失败（目标断言即省略号/固定高度），
    修复后 3/3 passed，锁定两个位置不再出现截断样式
  - 前端全量 `vitest run`：632 passed / 10 skipped
  - 浏览器布局实测（真实 SFC 样式 + 839px 内容区）：chip 189×48，名称 `clientWidth=scrollWidth=130`；
    记录行名称 `clientWidth=scrollWidth=125`（单行），应用栏 `scrollWidth=clientWidth=839`，无横向溢出
  - 安装版 WebView2 截图复验：deferred（需重新出包）
- 隐私检查：未包含设备身份、语音内容、个人路径或凭据；截图为用户提供的本机界面
