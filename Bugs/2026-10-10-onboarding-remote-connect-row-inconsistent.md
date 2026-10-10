# 向导②步「控制按键已收到」已打勾，但「遥控器已连接」没有选上

- 发现日期：2026-10-10（他机现场反馈，转交本仓库定位）
- 状态：已修复（自动化 passed；真机/现场复核 deferred）
- 影响范围：0.8.6（`source_revision=3ec6656`，origin/main）首次设置向导第②步；
  报告机器 Windows 10.0.22631；遥控器型号未读回（`model=unknown`）；现场无日志，
  只有截图 + 「复制诊断信息」块的文本
- 功能点：Onboarding 第②步「连接遥控器」右栏「连接检查」两行
- 现象：右栏「控制按键已收到 ✓」而「遥控器已连接 ○」；左卡同时写着「这一项可以
  继续了」，页脚却写「先连接遥控器，再继续。」——两句读起来互相打架
- 复现条件：系统侧（Windows 蓝牙设置）完成配对后，在②步按一下普通按键，但**没有
  在向导里**点「扫描已配对设备」→ 选中设备「连接」；应用自己的会话因此从未建立
- 正常预期：两条链路的事实都要如实显示（按键通道可用 ≠ 应用已连接），但界面不应
  用一句"可以继续了"去引导一个仍被阻断的步骤，也不应让用户以为"按键通了=遥控器连上了"
- 证据：现场诊断块关键行 `Remote: phase=idle model=unknown voice_ready=no
  input_phase=ready reconnects=0`（应用会话从未建立；输入通道在位）+ 截图
  （按键行 ✓ / 连接行 ○ / 页脚 `remote.not_connected`）。现场无 `sayall-diagnostic.log`
  可拉取，结论由代码路径 + 诊断块字段对齐得出
- 根因（两条链路不同源）：
  1. 「控制按键已收到」= 本向导会话观察到普通按键边沿；边沿来自 Windows 输入通道
     （`crates/sayall-windows/src/raw_input_windows.rs` 绑定系统里遥控器的 HID 接口，
     `src-tauri/src/lib.rs` 随应用启动即运行）——系统侧配对完成就能收到，
     **不要求应用自己的会话已建立**；
  2. 「遥控器已连接」= 应用自己的 BLE/ATVV 会话（`src/onboarding/panel.ts`：
     `connectionPhase ∈ {ready, streaming, draining} 且 bleVoiceReady`）；
     会话只在这两处建立：向导/连接页点设备「连接」（`src-tauri/src/lib.rs` 的
     `connect_remote`，顺带保存设备），或启动时恢复上次保存的设备——全新安装 +
     首次向导没有保存的设备，不会自动连；
  3. 文案把"按键已收到"写成"这一项可以继续了"（`RemoteStep.vue`），与门禁
     （`src/onboarding/flow.ts` 先查连接再查按键）给出的"先连接遥控器"直接冲突；
  4. 诊断块里没有②步按键观察这一事实（只有⑥步的 `Buttons observed: 0/3` 计数），
     现场只能靠截图拼形状
- 修复（最小改动，判据不变：连接 + 监听 + 按键三条仍都必须满足）：
  1. `src/onboarding/steps/RemoteStep.vue`：按键行说明分两态——已连接仍写
     "这一项可以继续了"；未连接改写"按键已收到。还差一步：点“扫描已配对设备”
     选中遥控器连接。"；
  2. `src/pages/OnboardingPage.vue`：进入②步、或停留在②步时系统侧遥控器刚出现
     （输入通道变为在位）后，若恰好只有一个已配对遥控器就自动扫描并连接
     （候选不唯一/未配对/已连接/连接中/重连中/用户已手动断开都不触发；自动与手动
     走同一条 `connectDevice`，日志 `detail=auto|manual`，扫描
     `auto_found_N` / `manual_found_N`）；
  3. `src/onboarding/diagnostics-text.ts`：诊断块补 `button_observed`、连接/音频
     `error=yes|no`、新 `Raw input:` 行（phase/matched/raw_events/semantic_edges/
     stale_events）与 `Voice test:` 行（phase/terminal）
- 验证：
  - `pnpm test`：先红后绿，299 passed | 11 skipped（新增：②步两态文案 2 例、
    自动连接 4 例、诊断块 2 例）；`pnpm build`（vue-tsc --noEmit + vite build）passed；
  - **deferred**：真机/现场复核——需要一台只有一个已配对遥控器的机器，
    按 `Testing/WindowsOnboardingWizard.md` 用例二（自动连接 + 两态文案）与
    「诊断信息」一节核对新字段；RC001 与 RC003 分别执行
- 隐私检查：本文档与新增日志/诊断字段只含稳定 token、计数与布尔；不含设备 id/名称、
  蓝牙地址、HID 路径、端点 id/名称、文件路径与任何输入内容
