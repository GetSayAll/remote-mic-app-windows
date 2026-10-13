# 向导②步：点「连接」后说明卡闪现；连接成功留下过期进度消息

- 发现日期：2026-10-13（Andy 真机截图反馈）
- 状态：已修复（自动化 passed；真机复核 deferred）
- 影响范围：0.8.6 及本分支的向导第②步（`RemoteStep` 说明卡 + 连接过程消息）
- 功能点：Onboarding 第②步的显示逻辑
- 现象：① 已连接状态下点设备卡「连接」，「首次连接遥控器」说明卡与
  「扫描已配对设备 / 打开蓝牙设置」按钮会**出现约 1–3 秒再消失**（闪现）；
  ② 连接成功后留下一句「正在确认语音功能，就绪后按一下普通按键。」——与下方
  按键状态行的引导重复，且在按键已收到后成为过期信息（截图里两者并存）
- 复现条件：应用已连接（或保存设备被启动恢复）→ 进②步 → 点设备卡「连接」
  （任何会经过 connecting 阶段的操作都一样，例如自动连接）
- 正常预期：首次连接说明卡只在「未连接、且没有在处理连接」时出现；连接过程由
  连接检查卡片与设备卡 trailing（「正在连接…」）表达；连接成立后过程消息即时退场
- 证据：用户截图（已连接状态：说明卡与扫描按钮均不可见、红框处为进度消息）；
  代码：`RemoteStep` 的卡片判据只有 `v-if="!connected"`，而 `connected` 只认
  `ready / streaming / draining`——点「连接」后应用要经过
  `connecting → awaiting_capabilities`（实测约 1–3 秒，见 2026-10-13 本机日志
  `ble_connect … elapsed_ms=2743`），这段窗口里 `connected=false`，卡片因此重新出现
- 根因：① 卡片判据没有排除「连接中」；② 成功路径把一句进度消息写入
  `operationMessage`，且没有任何地方在连接成立时清理它
- 修复：
  1. `src/onboarding/steps/RemoteStep.vue` 新增 `connectionInFlight`
     （connecting / discovering / awaiting_capabilities / reconnecting / suspended，
     或页面正在扫描 / 正在连接某个设备）；卡片判据改为
     `!connected && !connectionInFlight`；
  2. `src/pages/OnboardingPage.vue` 的 `connectDevice` 成功路径不再写进度消息
     （失败仍显示原因）；
  3. 同文件新增 watcher：连接进入 ready / streaming / draining 时清空
     `operationMessage`（清掉「已打开系统蓝牙设置…」这类残留的过程消息）
- 验证：先红后绿——`pnpm vitest run src/pages/OnboardingPage.test.ts`（44 passed；
  新增 3 例：连接中说明卡收起、成功不写进度消息、就绪后清残留消息；另更新 2 处
  既有断言），全量 `pnpm test` 304 passed | 11 skipped，`pnpm build`
  （vue-tsc --noEmit + vite build）passed。**deferred**：真机复核——②步点「连接」
  不应再闪现说明卡；连接后不应出现「正在确认语音功能…」那行
- 隐私检查：无设备身份、路径或输入内容
- 后续（同日第三轮，Andy 截图）：按现场要求把「首次连接遥控器」说明卡**整体撤掉**——
  首次配对说明整段移入第①步的强调卡；第②步的「扫描已配对设备 / 打开蓝牙设置」改为
  **只在没扫到任何已配对遥控器时**显示，列表有条目后由设备行接管（连接做成按钮观感、
  成功后显示「已连接」）。本节第 1 条的 `connectionInFlight` 判据连同该卡一起删除；
  本记录描述的"闪现窗口"已不存在。现场口径与验收项见
  `docs/product-copy.md` §7 的 2026-10-13 第三轮条目与 `Testing/WindowsOnboardingWizard.md`
  用例二第 1–7 项。
