# VB-CABLE 缺失导致无法语音输入

## 复现证据

用户在 Windows 真机成功安装并连接 RC001；选择扬声器端点后，按住语音键能从扬声器听到遥控器音频，但系统没有 `CABLE Input`，因此不能把音频送入语音转文字软件。页面同时显示 BLE 会话和 WASAPI 已就绪。

## 日志与状态结论

现场没有提供原始运行日志。页面的 BLE 和 WASAPI 就绪状态只证明 RC001 音频已进入所选扬声器端点，不能证明系统存在虚拟音频驱动或语音转文字链路可用；缺少 `CABLE Input` 与安装包未提供 VB-CABLE 的实现一致。

## 根因

应用当前只枚举并初始化系统中已经存在的 Windows 输出端点，不会创建虚拟音频设备。VB-CABLE 需要安装内核驱动并通常要求管理员确认和重启。

## 许可与安装边界

VB-CABLE Pack45 内附许可要求集成到其他安装流程前取得作者同意；VB-Audio 当前官网另有带 Donationware 条件的分发说明。当前方案不复制、捆绑、下载或执行驱动包，只把用户带到 VB-Audio 官方页面，因此不依赖解决两份措辞差异。

## 修复

- SayAll 可见安装完成后检查 `VBAudioVACMME` 驱动服务；未安装时说明第三方来源、Donationware、管理员权限和重启要求，并可打开官方下载页。
- 静默安装、升级和 CI 不自动打开网页。
- 应用首次进入连接页时自动枚举音频端点；只有一个 VB-CABLE 候选且尚未保存其他端点时自动选择。
- 未检测到时提供官方下载和重新检测入口；已保存的用户端点不被覆盖。

## 验证

- `pnpm test`：15 项前端测试通过，覆盖唯一 CABLE Input 自动选择、已保存端点不覆盖和缺失安装入口。
- `pnpm build`：TypeScript 检查和生产前端构建通过。
- `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo check --workspace`：格式、60 项 Rust 测试和工作区检查通过。
- `pnpm tauri build --no-bundle`：桌面程序构建通过，新增 opener capability 可被 Tauri 解析。
- Windows WebView 仿真流程已更新为覆盖首次检测并自动选择唯一仿真 CABLE Input，待本提交的 Windows CI 执行。
- Windows bundle 校验已增加安装器服务键、官方下载 URL 和静默模式不弹窗门禁，待本提交的 Windows CI 执行。
- 可见安装提示、真实 VB-CABLE 安装、UAC、重启后端点出现和语音转文字仍需 Windows 真机验收。

## 验证边界

RC001 → PCM → WASAPI 扬声器输出已由用户真机观察到；VB-CABLE 回环和语音转文字仍为 deferred。
