# 连接页快捷键录入：松键自动保存导致输入法和弦被截断

- 发现日期：2026-09-28
- 状态：已修复（失焦根因由 Windows 真机日志确认；修复包复测 deferred）
- 影响范围：连接页「修改快捷键」；不改按住说话注入时序、BLE/ATVV 生命周期或按键页录入
- 现象：录入微信输入法现用和弦时，外部输入法可能只放行半截物理边沿；旧实现等待 200ms 后自动保存，结果可能只剩 `左 Ctrl`。连接页录入因此被用户判定不可用。
- 根因：旧状态机把「所有键已松开」当成提交条件，并用固定稳定窗口等待第三方重放；这无法证明草稿已经完整。后续 ConsentStore 语音观测还会把半截输入猜成默认组合，无法表达用户真实配置。
- 预期：松键只更新草稿，不保存；用户可在同一会话继续补键，确认无误后显式保存。单独 `右 Alt` 合法；`Win+L`、`Ctrl+Alt+Del` 不写入连接页设置。
- 修复：
  - 录入会话内按首次按下顺序持续去重累计按键，释放沿不再触发定稿；
  - 增加「清空 / 取消 / 保存」；超时取消且不保存草稿；
  - 删除 200ms 自动定稿和 ConsentStore 和弦推断；第三方注入副本只作为普通边沿去重合并；
  - 保存前拒绝 Windows 保留组合；保存或停止失败保持明确错误状态，并补充结构化前端事件。
- 自动化验证：
  - TDD 红灯：新增显式保存与保留组合用例时，旧实现分别发生提前保存和写入 `Win+L`；
  - `pnpm exec vitest run src/pages/ConnectionPage.test.ts --reporter=verbose`：17/17 passed；
  - `pnpm test`：13 files、134/134 passed；
  - `pnpm build`：`vue-tsc --noEmit` 与 Vite production build passed；
  - `git diff --check`：passed。
- 可视验证：浏览器预览的连接页静态布局 passed；浏览器模式按设计禁用系统级快捷键录入。`pnpm tauri:dev` 冷构建在资源检查处因 worktree 缺少 `src-tauri/sayall-helper.exe` 失败，未启动原生窗口，因此 Windows 键盘、微信输入法和豆包输入法真机复测均为 deferred。
- 隐私：新增日志只记录阶段、结果、原因和按键数量，不记录设备身份、个人路径、语音内容或具体用户输入文本。

## 2026-09-28 真机复测：输入法抢焦点导致草稿被取消

- 用户在本地安装包 `d4b970ae89480782472ba14c844b0fe255ac439d` 上反馈大多数录入失败。
- 诊断日志证明原生捕获正常启动并收到完整边沿；典型失败会话依次收到
  `LeftWindows/RightAlt` 的 DOWN/UP，随后前端以 `reason=window_blurred` 停止会话。
  另两次失败同时记录 `wetype_voice=observed`，说明目标快捷键触发输入法并抢走
  前台焦点后，连接页自己的失焦保护把已经正确录入的草稿丢弃。
- 修复：失焦后等待 300ms 让原生边沿投递进 WebView；已有草稿时保留会话并提示
  用户返回应用继续补按或保存，只有空草稿持续失焦才取消。窗口在等待期重新获得
  焦点则撤销取消计时，停止、卸载组件和新会话均成对清理计时器。
- TDD：新增两条回归用例。旧实现中「有草稿失焦」用例因调用
  `stopShortcutCapture` 红灯；修复后「有草稿保留且可保存」与「空草稿仍取消」均通过。
- 自动化验证：`pnpm test -- --run src/pages/ConnectionPage.test.ts`：13 files、
  136/136 passed（其中连接页 19/19）。
- 新日志终态：`window_blurred_draft_preserved` 与 `window_blurred_empty` 可直接区分
  抢焦点后保留和空会话取消，不新增用户输入内容。
